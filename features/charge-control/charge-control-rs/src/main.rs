//! charge-control: 基于电量的充电控制守护程序（Rust 版）
//!
//! 适用于小米平板 5（nabu）上的 LIONSEMI LN8000 充电芯片。
//! - 电量 > --stop（默认 75）时停止充电；
//! - 电量 < --start（默认 50）时允许充电；
//! - 中间区间保持当前状态，避免反复切换。
//!
//! 内核驱动未把充电开关暴露为 sysfs 接口，本程序直接通过 /dev/i2c-1
//! 读写 LN8000 SYS_CTRL 寄存器的 STANDBY_EN(bit3) / EN_1TO1(bit0) 位，
//! 与驱动内部 `psy_chg_set_charging_enable()` 的寄存器操作一致。
//!
//! 每次决策都以硬件寄存器实际值重新断言，并监测充电器接入状态变化，
//! 因此拔插电源后驱动恢复的充电会在约一个监测周期内被重新按阈值接管。

use std::env;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const I2C_M_RD: u16 = 0x0001;
const I2C_RDWR: libc::c_ulong = 0x0707;

const LN8000_ADDR: u16 = 0x51;
const REG_DEVICE_ID: u8 = 0x00;
const REG_SYS_STS: u8 = 0x03;
const REG_SYS_CTRL: u8 = 0x1E;
const EXPECTED_DEVICE_ID: u8 = 0x42;

const STANDBY_BIT: u8 = 1 << 3;
const EN1TO1_BIT: u8 = 1 << 0;
const CTRL_MASK: u8 = STANDBY_BIT | EN1TO1_BIT;

#[repr(C)]
struct I2cMsg {
    addr: u16,
    flags: u16,
    len: u16,
    buf: *mut u8,
}

#[repr(C)]
struct I2cRdwr {
    msgs: *mut I2cMsg,
    nmsgs: u32,
}

static STOP_FLAG: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_signal(_sig: libc::c_int) {
    STOP_FLAG.store(true, Ordering::SeqCst);
}

struct Config {
    stop: i32,
    start: i32,
    interval: Duration,
    watch_interval: Duration,
    battery: PathBuf,
    charger_psy: PathBuf,
    i2c_dev: String,
    i2c_addr: u16,
    once: bool,
    status: bool,
    dry_run: bool,
    no_restore_on_exit: bool,
    verbose: bool,
}

fn usage() -> ! {
    println!(
        "charge-control: 电量阈值充电控制（LN8000 / 小米平板 5）

用法: charge-control [选项]

选项:
  --stop <N>               停止充电电量阈值（默认 75）
  --start <N>              允许充电电量阈值（默认 50）
  --interval <N>           电量常规检查间隔秒数（默认 20，最小 5）
  --watch-interval <N>     充电器接入状态监测间隔秒数（默认 2，最小 1）
  --battery <PATH>         电池 sysfs 目录（默认自动查找）
  --charger-psy <PATH>     充电器 power_supply 目录（默认 ln8000-charger）
  --i2c-dev <DEV>          I2C 设备（默认 /dev/i2c-1）
  --i2c-addr <ADDR>        LN8000 芯片地址，支持 0x51 或 81（默认 0x51）
  --once                   只执行一次检查后退出
  --status                 只读显示当前状态后退出
  --dry-run                只打印动作，不写寄存器
  --no-restore-on-exit     退出时保持当前状态（默认恢复允许充电）
  --verbose                输出更详细日志
  -h, --help               显示帮助"
    );
    process::exit(0);
}

fn next_arg(args: &[String], i: &mut usize, name: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("选项 {} 缺少参数", name))
}

fn parse_int(s: &str, name: &str) -> Result<i32, String> {
    s.parse::<i32>().map_err(|_| format!("选项 {} 参数无效: {}", name, s))
}

fn parse_addr(s: &str) -> Result<u16, String> {
    let v = if let Some(hex) = s.strip_prefix("0x") {
        u16::from_str_radix(hex, 16)
    } else {
        s.parse::<u16>()
    };
    v.map_err(|_| format!("芯片地址无效: {}", s))
}

fn parse_args() -> Result<Config, String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut stop = 75i32;
    let mut start = 50i32;
    let mut interval = 20u64;
    let mut watch_interval = 2u64;
    let mut battery: Option<String> = None;
    let mut charger_psy = "/sys/class/power_supply/ln8000-charger".to_string();
    let mut i2c_dev = "/dev/i2c-1".to_string();
    let mut i2c_addr = LN8000_ADDR;
    let mut once = false;
    let mut status = false;
    let mut dry_run = false;
    let mut no_restore_on_exit = false;
    let mut verbose = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--stop" => stop = parse_int(&next_arg(&args, &mut i, "--stop")?, "--stop")?,
            "--start" => start = parse_int(&next_arg(&args, &mut i, "--start")?, "--start")?,
            "--interval" => interval = parse_int(&next_arg(&args, &mut i, "--interval")?, "--interval")? as u64,
            "--watch-interval" => watch_interval = parse_int(&next_arg(&args, &mut i, "--watch-interval")?, "--watch-interval")? as u64,
            "--battery" => battery = Some(next_arg(&args, &mut i, "--battery")?),
            "--charger-psy" => charger_psy = next_arg(&args, &mut i, "--charger-psy")?,
            "--i2c-dev" => i2c_dev = next_arg(&args, &mut i, "--i2c-dev")?,
            "--i2c-addr" => i2c_addr = parse_addr(&next_arg(&args, &mut i, "--i2c-addr")?)?,
            "--once" => once = true,
            "--status" => status = true,
            "--dry-run" => dry_run = true,
            "--no-restore-on-exit" => no_restore_on_exit = true,
            "--verbose" => verbose = true,
            "-h" | "--help" => usage(),
            other => return Err(format!("未知选项: {}", other)),
        }
        i += 1;
    }

    if stop <= start {
        return Err("--stop 必须大于 --start".into());
    }
    if !(0..=100).contains(&start) || !(0..=100).contains(&stop) {
        return Err("阈值必须在 0-100 之间且 --start < --stop".into());
    }
    if interval < 5 {
        return Err("--interval 不能小于 5 秒".into());
    }
    if watch_interval < 1 {
        return Err("--watch-interval 不能小于 1 秒".into());
    }

    Ok(Config {
        stop,
        start,
        interval: Duration::from_secs(interval),
        watch_interval: Duration::from_secs(watch_interval),
        battery: find_battery(battery.as_deref())?,
        charger_psy: PathBuf::from(charger_psy),
        i2c_dev,
        i2c_addr,
        once,
        status,
        dry_run,
        no_restore_on_exit,
        verbose,
    })
}

fn find_battery(explicit: Option<&str>) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        let path = PathBuf::from(p);
        if path.join("capacity").exists() {
            return Ok(path);
        }
        return Err(format!("电池路径不存在: {}", p));
    }
    let base = Path::new("/sys/class/power_supply");
    let entries = fs_entries(base)?;
    for name in entries {
        let psy = base.join(&name);
        let is_battery = read_sysfs(&psy.join("type")).as_deref() == Some("Battery");
        if is_battery && psy.join("capacity").exists() {
            return Ok(psy);
        }
    }
    Err("未找到电池节点，请用 --battery 指定".into())
}

fn fs_entries(dir: &Path) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| format!("读取 {} 失败: {}", dir.display(), e))? {
        let entry = entry.map_err(|e| format!("读取目录项失败: {}", e))?;
        out.push(entry.file_name().to_string_lossy().into_owned());
    }
    out.sort();
    Ok(out)
}

fn read_sysfs(path: &Path) -> Option<String> {
    let mut s = String::new();
    File::open(path).ok()?.read_to_string(&mut s).ok()?;
    Some(s.trim().to_string())
}

fn read_capacity(battery: &Path) -> i32 {
    read_sysfs(&battery.join("capacity"))
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(-1)
}

fn log_line(level: &str, msg: &str) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let t = now.as_secs() as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::localtime_r(&t, &mut tm);
    }
    let fmt = b"%Y-%m-%d %H:%M:%S\0";
    let mut buf = [0u8; 64];
    let n = unsafe {
        libc::strftime(
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            fmt.as_ptr() as *const libc::c_char,
            &tm,
        )
    };
    let ts = String::from_utf8_lossy(&buf[..n]);
    println!("{} {} {}", ts, level, msg);
}

fn info(msg: &str) {
    log_line("INFO", msg);
}

fn debug(cfg: &Config, msg: &str) {
    if cfg.verbose {
        log_line("DEBUG", msg);
    }
}

fn error(msg: &str) {
    log_line("ERROR", msg);
}

fn mode_name(sys_sts: u8) -> &'static str {
    if sys_sts & 0x08 != 0 {
        "bypass"
    } else if sys_sts & 0x04 != 0 {
        "switching(充电)"
    } else if sys_sts & 0x02 != 0 {
        "standby(停止)"
    } else if sys_sts & 0x01 != 0 {
        "shutdown"
    } else {
        "unknown"
    }
}

struct Ln8000 {
    file: File,
    addr: u16,
}

impl Ln8000 {
    fn open(dev: &str, addr: u16) -> std::io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(dev)?;
        Ok(Self { file, addr })
    }

    fn read_reg(&mut self, reg: u8) -> std::io::Result<u8> {
        let mut reg_buf = reg;
        let mut out = 0u8;
        let mut msgs = [
            I2cMsg {
                addr: self.addr,
                flags: 0,
                len: 1,
                buf: &mut reg_buf as *mut u8,
            },
            I2cMsg {
                addr: self.addr,
                flags: I2C_M_RD,
                len: 1,
                buf: &mut out as *mut u8,
            },
        ];
        let mut data = I2cRdwr {
            msgs: msgs.as_mut_ptr(),
            nmsgs: 2,
        };
        let rc = unsafe { libc::ioctl(self.file.as_raw_fd(), I2C_RDWR, &mut data) };
        if rc < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(out)
        }
    }

    fn write_reg(&mut self, reg: u8, val: u8) -> std::io::Result<()> {
        let mut buf = [reg, val];
        let mut msgs = [I2cMsg {
            addr: self.addr,
            flags: 0,
            len: 2,
            buf: buf.as_mut_ptr(),
        }];
        let mut data = I2cRdwr {
            msgs: msgs.as_mut_ptr(),
            nmsgs: 1,
        };
        let rc = unsafe { libc::ioctl(self.file.as_raw_fd(), I2C_RDWR, &mut data) };
        if rc < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn check_device(&mut self) -> bool {
        self.read_reg(REG_DEVICE_ID)
            .map(|v| v == EXPECTED_DEVICE_ID)
            .unwrap_or(false)
    }

    fn sys_sts(&mut self) -> std::io::Result<u8> {
        self.read_reg(REG_SYS_STS)
    }

    fn sys_ctrl(&mut self) -> std::io::Result<u8> {
        self.read_reg(REG_SYS_CTRL)
    }

    fn set_charging(&mut self, enabled: bool) -> std::io::Result<u8> {
        let cur = self.sys_ctrl()?;
        let new = if enabled {
            cur & !CTRL_MASK
        } else {
            (cur & !CTRL_MASK) | STANDBY_BIT
        };
        if new != cur {
            self.write_reg(REG_SYS_CTRL, new)?;
        }
        Ok(new)
    }
}

fn power_available(cfg: &Config) -> (bool, String) {
    if read_sysfs(&cfg.charger_psy.join("present")).as_deref() == Some("1") {
        return (true, format!("{} present=1", cfg.charger_psy.display()));
    }
    let base = Path::new("/sys/class/power_supply");
    if let Ok(entries) = fs_entries(base) {
        for name in entries {
            let psy = base.join(&name);
            let ty = read_sysfs(&psy.join("type")).unwrap_or_default();
            if ty != "USB" && ty != "Mains" {
                continue;
            }
            if read_sysfs(&psy.join("online")).as_deref() == Some("1") {
                return (true, format!("{} online=1", psy.display()));
            }
        }
    }
    (false, "无电源接入".to_string())
}

fn print_status(cfg: &Config) -> Result<(), String> {
    let mut ln = Ln8000::open(&cfg.i2c_dev, cfg.i2c_addr)
        .map_err(|e| format!("打开 {} 失败: {}", cfg.i2c_dev, e))?;
    let chip_id = ln
        .read_reg(REG_DEVICE_ID)
        .map_err(|e| format!("读取芯片 ID 失败: {}", e))?;
    let sts = ln.sys_sts().map_err(|e| format!("读取 SYS_STS 失败: {}", e))?;
    let ctrl = ln.sys_ctrl().map_err(|e| format!("读取 SYS_CTRL 失败: {}", e))?;

    let get = |p: &str| read_sysfs(&cfg.battery.join(p)).unwrap_or_else(|| "?".into());
    let getc = |p: &str| read_sysfs(&cfg.charger_psy.join(p)).unwrap_or_else(|| "?".into());

    println!("芯片 ID : 0x{:02X} (期望 0x{:02X})", chip_id, EXPECTED_DEVICE_ID);
    println!("电池    : {}", cfg.battery.display());
    println!("  电量    : {}%", get("capacity"));
    println!("  状态    : {}", get("status"));
    println!("  电流    : {} uA", get("current_now"));
    println!("充电器  : {}", cfg.charger_psy.display());
    let (power_ok, power_src) = power_available(cfg);
    println!("  接入    : {}", getc("present"));
    println!("  状态    : {}", getc("status"));
    println!("  电源可用: {}", if power_ok { "是" } else { "否" });
    println!("  来源    : {}", power_src);
    println!("SYS_STS : 0x{:02X} -> {}", sts, mode_name(sts));
    println!(
        "SYS_CTRL: 0x{:02X} -> {}",
        ctrl,
        if ctrl & STANDBY_BIT != 0 {
            "STANDBY 已置位(停止充电)"
        } else {
            "未停止"
        }
    );
    Ok(())
}

fn decide(cfg: &Config, cap: i32, power_ok: bool) -> Option<&'static str> {
    if cap > cfg.stop {
        Some("stop")
    } else if cap < cfg.start && power_ok {
        Some("allow")
    } else {
        None
    }
}

fn desired_ctrl(cur: u8, action: &str) -> u8 {
    match action {
        "stop" => (cur & !CTRL_MASK) | STANDBY_BIT,
        "allow" => cur & !CTRL_MASK,
        _ => cur,
    }
}

fn enforce(cfg: &Config, ln: &mut Ln8000, cap: i32, power_ok: bool, reason: &str) {
    let action = match decide(cfg, cap, power_ok) {
        Some(a) => a,
        None => {
            debug(
                cfg,
                &format!("电量 {}% 在 {}%~{}% 之间，保持当前状态", cap, cfg.start, cfg.stop),
            );
            return;
        }
    };
    if action == "allow" && !power_ok {
        info(&format!("电量 {}% < {}%，但电源未接入，跳过", cap, cfg.start));
        return;
    }
    let cur = match ln.sys_ctrl() {
        Ok(v) => v,
        Err(e) => {
            error(&format!("读取 SYS_CTRL 失败: {}", e));
            return;
        }
    };
    let new = desired_ctrl(cur, action);
    if new == cur {
        debug(
            cfg,
            &format!("电量 {}%，目标 {}，硬件已处于该状态（SYS_CTRL=0x{:02X}）", cap, action, cur),
        );
        return;
    }
    let suffix = if reason.is_empty() {
        String::new()
    } else {
        format!("（{}）", reason)
    };
    let action_text = if action == "stop" {
        format!("> {}%，停止充电", cfg.stop)
    } else {
        format!("< {}%，允许充电", cfg.start)
    };
    if cfg.dry_run {
        info(&format!(
            "[dry-run] 电量 {}% {}{}：将切换 SYS_CTRL 0x{:02X} -> 0x{:02X}",
            cap, action_text, suffix, cur, new
        ));
        return;
    }
    if let Err(e) = ln.write_reg(REG_SYS_CTRL, new) {
        error(&format!("写入 SYS_CTRL 失败: {}", e));
        return;
    }
    info(&format!(
        "电量 {}% {}{}（SYS_CTRL 0x{:02X} -> 0x{:02X}）",
        cap, action_text, suffix, cur, new
    ));
}

fn restore_charging(cfg: &Config, ln: &mut Ln8000) {
    let (power_ok, power_src) = power_available(cfg);
    if power_ok {
        match ln.set_charging(true) {
            Ok(new) => info(&format!("收到退出信号，已恢复允许充电（SYS_CTRL=0x{:02X}）", new)),
            Err(e) => error(&format!("退出时恢复充电失败: {}", e)),
        }
    } else {
        info(&format!("收到退出信号，电源未接入（{}），不干预", power_src));
    }
}

fn main() {
    let cfg = match parse_args() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("参数错误: {}", e);
            eprintln!("使用 --help 查看帮助");
            process::exit(2);
        }
    };

    if cfg.status {
        if let Err(e) = print_status(&cfg) {
            eprintln!("{}", e);
            process::exit(1);
        }
        return;
    }

    if unsafe { libc::geteuid() } != 0 {
        eprintln!("必须以 root 运行（/dev/i2c-* 仅 root 可访问），推荐以 systemd 服务方式运行");
        process::exit(1);
    }

    let mut ln = match Ln8000::open(&cfg.i2c_dev, cfg.i2c_addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("打开 {} 失败: {}", cfg.i2c_dev, e);
            process::exit(1);
        }
    };
    if !ln.check_device() {
        eprintln!(
            "I2C 设备校验失败：{} 地址 0x{:02X} 上未找到 LN8000（期望 ID 0x{:02X}）",
            cfg.i2c_dev, cfg.i2c_addr, EXPECTED_DEVICE_ID
        );
        process::exit(1);
    }

    info(&format!(
        "LN8000 校验通过，电量阈值: 停止 >{}%，允许 <{}%，轮询间隔 {}s，插拔监测间隔 {}s",
        cfg.stop,
        cfg.start,
        cfg.interval.as_secs(),
        cfg.watch_interval.as_secs()
    ));

    unsafe {
        libc::signal(libc::SIGTERM, handle_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, handle_signal as *const () as libc::sighandler_t);
    }

    let mut last_power: Option<bool> = None;
    let mut last_cap_check: Option<Instant> = None;

    loop {
        if STOP_FLAG.load(Ordering::SeqCst) {
            if !cfg.no_restore_on_exit {
                restore_charging(&cfg, &mut ln);
            } else {
                info("收到退出信号，按配置保持当前状态退出");
            }
            break;
        }

        let (power_ok, power_src) = power_available(&cfg);

        // 电源插拔：驱动可能漏记 ln8000 present，综合 present 与 USB/Mains online 判断
        if let Some(last_ok) = last_power {
            if last_ok != power_ok {
                info(&format!(
                    "电源接入状态变化: {} -> {}（{}）",
                    if last_ok { "已接入" } else { "未接入" },
                    if power_ok { "已接入" } else { "未接入" },
                    power_src
                ));
                let cap = read_capacity(&cfg.battery);
                if cap < 0 {
                    error("读取电量失败，等待下个周期重试");
                } else {
                    enforce(&cfg, &mut ln, cap, power_ok, "电源插拔");
                }
                last_cap_check = Some(Instant::now());
            }
        }

        let do_cap_check = match last_cap_check {
            None => true,
            Some(t) => t.elapsed() >= cfg.interval,
        };
        if do_cap_check {
            let cap = read_capacity(&cfg.battery);
            if cap < 0 {
                error("读取电量失败，等待下个周期重试");
            } else {
                enforce(&cfg, &mut ln, cap, power_ok, "周期检查");
            }
            last_cap_check = Some(Instant::now());
        }

        last_power = Some(power_ok);
        if cfg.once {
            break;
        }
        thread::sleep(cfg.watch_interval);
    }
}
