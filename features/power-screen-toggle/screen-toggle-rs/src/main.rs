//! Toggle the display backlight from the power button and the lid switch.
//!
//! Behavior:
//!   * Power button (KEY_POWER) short press: toggle screen off/on.
//!   * Lid close (SW_LID = 1): turn the screen off.
//!   * Lid open  (SW_LID = 0): turn the screen on.
//!
//! The previous brightness is remembered so the screen returns to the same
//! level when it is turned back on.
//!
//! This is a from-scratch Rust port of the original Python daemon. It uses
//! only the standard library plus a few raw libc declarations (open/read/
//! poll/signal/localtime), so it has zero crate dependencies.

use std::ffi::{CStr, CString};
use std::fs;
use std::io::{self, ErrorKind};
use std::mem;
use std::os::unix::process::CommandExt;
use std::os::raw::{c_char, c_int, c_long, c_short, c_ulong, c_void};
use std::os::unix::io::RawFd;
use std::process::{Command, Stdio};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const EV_KEY: u16 = 0x01;
const EV_REL: u16 = 0x02;
const EV_ABS: u16 = 0x03;
const EV_SW: u16 = 0x05;
const KEY_POWER: u16 = 116;
const SW_LID: u16 = 0;

const BACKLIGHT_ROOT: &str = "/sys/class/backlight";
const STATE_DIR: &str = "/var/lib/screen-toggle";
const STATE_FILE: &str = "/var/lib/screen-toggle/last-brightness";
const DISPLAY_SETTLE_MS: u64 = 700;
// Mutter reports the minimum backlight value for this panel as 20; GNOME's
// idle-dim percentage is applied on the min..max range.
const BACKLIGHT_MIN: i64 = 20;
// How long the panel stays blanked inside a recovery cycle before being
// unblanked again. Long enough for the NVT suspend sequence to finish, short
// enough that the forced post-resume cycle barely delays the visible wake.
const RECOVERY_PANEL_OFF_MS: u64 = 300;
// Extra blank/unblank cycles attempted after the first wake when the
// Novatek display/touch controller reports an init failure. Total unblank
// attempts = 1 (initial) + MAX_RECOVERY_CYCLES.
const MAX_RECOVERY_CYCLES: u32 = 2;

// Suspend countdowns started when we turn the screen off.
const POWER_SUSPEND_DELAY_SECS: f64 = 15.0;
const LID_SUSPEND_DELAY_SECS: f64 = 1800.0;
// How often to rescan input devices so keyboards/mice that connect later
// (e.g. Bluetooth) are picked up for wake.
const INPUT_RESCAN_INTERVAL_SECS: f64 = 3.0;
// Lid-close wake loop protection: re-suspend at most this many times inside
// one window, then stay awake with the screen off.
const MAX_LID_RESUSPENDS: u32 = 3;
const RESUSPEND_WINDOW_SECS: f64 = 60.0;

const POINTER_PROPS_TOUCH: [&str; 2] = ["ID_INPUT_TOUCHSCREEN", "ID_INPUT_TABLET"];
const POINTER_PROPS_OTHER: [&str; 3] = [
    "ID_INPUT_MOUSE",
    "ID_INPUT_TOUCHPAD",
    "ID_INPUT_POINTINGSTICK",
];

// Kernel messages that indicate the display/touch controller failed to
// initialize (the "press power twice" workaround trigger on nabu).
const KERNEL_INIT_FAILURE_MARKERS: [&str; 8] = [
    "FW info is broken",
    "nvt_get_fw_info failed",
    "download firmware failed",
    "failed to initialize panel",
    "Failed to set pixel format",
    "Failed to set display on",
    "Failed to set tear on",
    "Failed to set exit sleep mode",
];

// ---------------------------------------------------------------------------
// Minimal libc FFI (no external crates).
// ---------------------------------------------------------------------------

type TimeT = i64;
type NfdsT = usize;

const O_RDONLY: c_int = 0;
const O_NONBLOCK: c_int = 0o4000;
const POLLIN: c_short = 0x0001;
const POLLERR: c_short = 0x0008;
const POLLHUP: c_short = 0x0010;

const SIGUSR1: c_int = 10;
const SIGTERM: c_int = 15;
const SIGINT: c_int = 2;
const SIGKILL: c_int = 9;

// EVIOCGSW(sizeof(u64)): read the current switch bitmap from an evdev fd.
const EVIOCGSW: c_ulong = 0x8008451B;

#[repr(C)]
#[derive(Clone, Copy)]
struct InputEvent {
    time_sec: i64,
    time_usec: i64,
    event_type: u16,
    code: u16,
    value: i32,
}

const EVENT_SIZE: usize = mem::size_of::<InputEvent>();

#[repr(C)]
#[derive(Clone, Copy)]
struct PollFd {
    fd: c_int,
    events: c_short,
    revents: c_short,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Tm {
    tm_sec: c_int,
    tm_min: c_int,
    tm_hour: c_int,
    tm_mday: c_int,
    tm_mon: c_int,
    tm_year: c_int,
    tm_wday: c_int,
    tm_yday: c_int,
    tm_isdst: c_int,
    tm_gmtoff: c_long,
    tm_zone: *const c_char,
}

extern "C" {
    fn open(path: *const c_char, flags: c_int, ...) -> c_int;
    fn close(fd: c_int) -> c_int;
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    fn kill(pid: c_int, sig: c_int) -> c_int;
    fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
    fn poll(fds: *mut PollFd, nfds: NfdsT, timeout: c_int) -> c_int;
    fn signal(signum: c_int, handler: usize) -> usize;
    fn localtime_r(timep: *const TimeT, result: *mut Tm) -> *mut Tm;
    fn strftime(
        s: *mut c_char,
        max: usize,
        format: *const c_char,
        tm: *const Tm,
    ) -> usize;
}

static SIGUSR1_FLAG: AtomicBool = AtomicBool::new(false);
static SIGTERM_FLAG: AtomicBool = AtomicBool::new(false);
static SIGINT_FLAG: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(true);

extern "C" fn on_sigusr1(_: c_int) {
    SIGUSR1_FLAG.store(true, Ordering::SeqCst);
}

extern "C" fn on_sigterm(_: c_int) {
    SIGTERM_FLAG.store(true, Ordering::SeqCst);
    RUNNING.store(false, Ordering::SeqCst);
}

extern "C" fn on_sigint(_: c_int) {
    SIGINT_FLAG.store(true, Ordering::SeqCst);
    RUNNING.store(false, Ordering::SeqCst);
}

// ---------------------------------------------------------------------------
// Small helpers.
// ---------------------------------------------------------------------------

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn local_tm(secs: TimeT) -> Tm {
    let mut tm: Tm = unsafe { mem::zeroed() };
    unsafe {
        localtime_r(&secs, &mut tm);
    }
    tm
}

fn format_epoch(epoch: f64) -> String {
    let secs = epoch.floor() as TimeT;
    let millis = ((epoch - epoch.floor()) * 1000.0).round() as i64;
    let tm = local_tm(secs);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        millis
    )
}

fn log(msg: &str) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as TimeT;
    let tm = local_tm(secs);
    let fmt = CString::new("%Y-%m-%d %H:%M:%S").unwrap();
    let mut buf = [0u8; 64];
    unsafe {
        strftime(
            buf.as_mut_ptr() as *mut c_char,
            buf.len(),
            fmt.as_ptr(),
            &tm,
        );
    }
    let ts = unsafe { CStr::from_ptr(buf.as_ptr() as *const c_char) };
    println!("{} {}", ts.to_string_lossy(), msg);
}

struct CmdResult {
    ok: bool,
    stdout: String,
    stderr: String,
}

/// Run a command and capture output.
fn run_cmd(cmd: &[&str], secs: u64) -> Option<CmdResult> {
    run_cmd_impl(cmd, None, secs)
}

/// Run a command as the given graphical user via runuser.
fn run_cmd_as_user(cmd: &[&str], user: &str, uid: u32, secs: u64) -> Option<CmdResult> {
    run_cmd_impl(cmd, Some((user, uid)), secs)
}

/// Run a command (optionally as a user) and capture its output.
///
/// `secs` is a real timeout: if the command has not finished after that many
/// seconds, its whole process group is killed and a failed result is
/// returned. This prevents a wedged D-Bus call (e.g. gdbus waiting on Mutter)
/// from blocking the daemon forever. `secs == 0` means no timeout.
fn run_cmd_impl(cmd: &[&str], as_user: Option<(&str, u32)>, secs: u64) -> Option<CmdResult> {
    let mut c = match as_user {
        Some((user, uid)) => {
            let mut c = Command::new("runuser");
            c.arg("-u").arg(user).arg("--").args(cmd);
            c.env("XDG_RUNTIME_DIR", format!("/run/user/{}", uid));
            c.env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path=/run/user/{}/bus", uid),
            );
            c
        }
        None => {
            let mut c = Command::new(cmd[0]);
            c.args(&cmd[1..]);
            c
        }
    };
    // Put the command in its own process group so a timeout can kill the
    // whole tree (e.g. runuser -> gdbus) instead of leaving an orphan behind.
    c.process_group(0);
    c.stdin(Stdio::null());
    c.stdout(Stdio::piped());
    c.stderr(Stdio::piped());
    let child = c.spawn().ok()?;
    let pid = child.id() as c_int;
    let (tx, rx) = mpsc::channel();
    let reaper = thread::spawn(move || {
        let out = child.wait_with_output();
        let _ = tx.send(());
        out
    });
    let out = if secs > 0 {
        match rx.recv_timeout(Duration::from_secs(secs)) {
            Ok(_) => reaper.join().ok()?.ok()?,
            Err(_) => {
                unsafe {
                    kill(-pid, SIGKILL);
                }
                let _ = reaper.join();
                return Some(CmdResult {
                    ok: false,
                    stdout: String::new(),
                    stderr: format!("timed out after {} s and was killed", secs),
                });
            }
        }
    } else {
        reaper.join().ok()?.ok()?
    };
    Some(CmdResult {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

fn open_nonblocking(path: &str) -> io::Result<RawFd> {
    let cpath = CString::new(path)
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "path contains NUL"))?;
    let fd = unsafe { open(cpath.as_ptr(), O_RDONLY | O_NONBLOCK) };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(fd)
    }
}

fn sorted_dir_names(path: &str) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(path)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

// ---------------------------------------------------------------------------
// Input device discovery (/proc/bus/input/devices).
// ---------------------------------------------------------------------------

#[derive(Default)]
struct InputDev {
    name: String,
    handlers: Vec<String>,
    ev: Vec<u64>,
    key: Vec<u64>,
    sw: Vec<u64>,
}

fn bitmap_has(words: &[u64], bit: u16) -> bool {
    let idx = bit as usize / 64;
    idx < words.len() && (words[idx] & (1u64 << (bit as u64 % 64))) != 0
}

fn parse_bitmap_words(s: &str) -> Vec<u64> {
    let mut words: Vec<u64> = s
        .split_whitespace()
        .filter_map(|w| u64::from_str_radix(w, 16).ok())
        .collect();
    // /proc/bus/input/devices prints words from the highest word down;
    // reverse them so word 0 covers the lowest bits.
    words.reverse();
    words
}

fn parse_input_devices() -> Vec<InputDev> {
    let mut devs = Vec::new();
    let mut cur = InputDev::default();
    let text = match fs::read_to_string("/proc/bus/input/devices") {
        Ok(t) => t,
        Err(e) => {
            log(&format!("cannot open /proc/bus/input/devices: {}", e));
            return devs;
        }
    };
    for line in text.lines() {
        if line.trim().is_empty() {
            if cur.name.is_empty() && cur.handlers.is_empty() {
                continue;
            }
            devs.push(mem::take(&mut cur));
            continue;
        }
        if let Some(v) = line.strip_prefix("N: Name=") {
            cur.name = v.to_string();
        } else if let Some(v) = line.strip_prefix("H: Handlers=") {
            cur.handlers = v.split_whitespace().map(str::to_string).collect();
        } else if let Some(v) = line.strip_prefix("B: EV=") {
            cur.ev = parse_bitmap_words(v);
        } else if let Some(v) = line.strip_prefix("B: KEY=") {
            cur.key = parse_bitmap_words(v);
        } else if let Some(v) = line.strip_prefix("B: SW=") {
            cur.sw = parse_bitmap_words(v);
        }
    }
    if !cur.name.is_empty() || !cur.handlers.is_empty() {
        devs.push(cur);
    }
    devs
}

fn event_node(dev: &InputDev) -> Option<String> {
    dev.handlers
        .iter()
        .find(|h| h.starts_with("event"))
        .map(|h| format!("/dev/input/{}", h))
}

// ---------------------------------------------------------------------------
// Daemon state.
// ---------------------------------------------------------------------------

struct Daemon {
    power_path: Option<String>,
    lid_path: Option<String>,
    power_fd: Option<RawFd>,
    lid_fd: Option<RawFd>,
    backlight: Option<String>,
    dpms_path: Option<String>,
    screen_off: bool,
    display_blanked: bool,
    last_toggle: f64,
    last_lid: Option<i32>,
    last_own_unblank: f64,
    suspend_deadline: Option<f64>,
    suspend_reason: Option<&'static str>,
    resuspend_count: u32,
    resuspend_window_start: f64,
    just_resumed: bool,
    user_brightness: i64,
    dim_brightness: Option<i64>,
    last_dim_check: f64,
    keyboard_fds: Vec<(RawFd, String)>,
    pointer_fds: Vec<(RawFd, String)>,
    last_input_rescan: f64,
    screen_on_at: f64,
    input_snapshot: String,
}

impl Daemon {
    fn new() -> Self {
        Daemon {
            power_path: None,
            lid_path: None,
            power_fd: None,
            lid_fd: None,
            backlight: None,
            dpms_path: None,
            screen_off: false,
            display_blanked: false,
            last_toggle: 0.0,
            last_lid: None,
            last_own_unblank: 0.0,
            suspend_deadline: None,
            suspend_reason: None,
            resuspend_count: 0,
            resuspend_window_start: 0.0,
            just_resumed: false,
            user_brightness: load_state(),
            dim_brightness: None,
            last_dim_check: 0.0,
            keyboard_fds: Vec::new(),
            pointer_fds: Vec::new(),
            last_input_rescan: 0.0,
            screen_on_at: 0.0,
            input_snapshot: String::new(),
        }
    }

    fn find_devices(&mut self) -> bool {
        for dev in parse_input_devices() {
            let Some(node) = event_node(&dev) else {
                continue;
            };
            if self.power_path.is_none() && bitmap_has(&dev.key, KEY_POWER) {
                self.power_path = Some(node.clone());
            }
            if self.lid_path.is_none()
                && bitmap_has(&dev.ev, EV_SW)
                && bitmap_has(&dev.sw, SW_LID)
            {
                self.lid_path = Some(node);
            }
        }
        self.power_path.is_some() || self.lid_path.is_some()
    }

    fn find_backlight(&mut self) -> bool {
        if self.backlight.is_some() {
            return true;
        }
        for name in sorted_dir_names(BACKLIGHT_ROOT) {
            let candidate = format!("{}/{}", BACKLIGHT_ROOT, name);
            let brightness = format!("{}/brightness", candidate);
            if fs::metadata(&brightness).map(|m| m.is_file()).unwrap_or(false) {
                self.backlight = Some(candidate);
                return true;
            }
        }
        false
    }

    fn read_brightness(&self) -> io::Result<i64> {
        let path = self
            .backlight
            .as_ref()
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "no backlight"))?;
        let raw = fs::read_to_string(format!("{}/brightness", path))?;
        raw.trim()
            .parse()
            .map_err(|_| io::Error::new(ErrorKind::InvalidData, "bad brightness value"))
    }

    fn read_max_brightness(&self) -> i64 {
        let path = match &self.backlight {
            Some(p) => p,
            None => return 2047,
        };
        fs::read_to_string(format!("{}/max_brightness", path))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(2047)
    }

    /// Cached GNOME idle-dim brightness (sysfs scale), refreshed every 60 s.
    /// None means dimming is disabled or the value could not be determined.
    fn idle_dim_value(&mut self) -> Option<i64> {
        if now_secs() - self.last_dim_check > 60.0 {
            self.dim_brightness = idle_dim_brightness(self.read_max_brightness());
            self.last_dim_check = now_secs();
            log(&format!(
                "idle-dim detection: {:?} (max {})",
                self.dim_brightness,
                self.read_max_brightness()
            ));
        }
        self.dim_brightness
    }

    /// While the screen is on, remember the brightness the user actually
    /// chose. GNOME's idle-dim value is detected (it applies
    /// `idle-brightness` percent over min..max) and ignored, so a later
    /// screen-on restores the real user brightness instead of the dimmed one.
    fn track_user_brightness(&mut self) {
        if self.screen_off {
            return;
        }
        let Ok(b) = self.read_brightness() else {
            return;
        };
        if b <= 0 {
            return;
        }
        if let Some(d) = self.idle_dim_value() {
            let tolerance = (self.read_max_brightness() / 50).max(5);
            if (b - d).abs() <= tolerance {
                return; // GNOME idle-dim value, not a user setting
            }
        }
        if b != self.user_brightness {
            self.user_brightness = b;
            save_state(b);
            log(&format!("tracked user brightness {}", b));
        }
    }

    fn write_brightness(&self, value: i64) -> io::Result<()> {
        // Keep Mutter's Backlight property in sync so the GNOME brightness
        // slider shows the real value. Mutter rejects values below its
        // minimum (20), so the screen-off write (0) still goes to sysfs.
        if value >= BACKLIGHT_MIN && set_compositor_backlight(value) {
            return Ok(());
        }
        let path = self
            .backlight
            .as_ref()
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "no backlight"))?;
        fs::write(format!("{}/brightness", path), value.to_string())
    }

    fn find_dpms_path(&mut self) -> Option<String> {
        if let Some(p) = &self.dpms_path {
            if fs::metadata(p).is_ok() {
                return Some(p.clone());
            }
        }
        self.dpms_path = None;
        let mut best = None;
        for name in sorted_dir_names("/sys/class/drm") {
            if !name.starts_with("card") || !name.contains('-') {
                continue;
            }
            let base = format!("/sys/class/drm/{}", name);
            if !fs::metadata(format!("{}/dpms", base))
                .map(|m| m.is_file())
                .unwrap_or(false)
            {
                continue;
            }
            let connected = fs::read_to_string(format!("{}/status", base))
                .map(|s| s.trim() == "connected")
                .unwrap_or(false);
            if connected {
                self.dpms_path = Some(format!("{}/dpms", base));
                return self.dpms_path.clone();
            }
            if best.is_none() {
                best = Some(format!("{}/dpms", base));
            }
        }
        self.dpms_path = best;
        self.dpms_path.clone()
    }

    fn read_dpms(&mut self) -> Option<String> {
        let path = self.find_dpms_path()?;
        fs::read_to_string(path).ok().map(|s| s.trim().to_string())
    }

    fn update_display_state(&mut self) {
        if let Some(state) = self.read_dpms() {
            self.display_blanked = state != "On";
        }
    }

    fn screen_visibly_on(&self) -> bool {
        if self.display_blanked {
            return false;
        }
        if self.backlight.is_some() {
            if let Ok(v) = self.read_brightness() {
                return v > 0;
            }
        }
        true
    }

    fn setup(&mut self) {
        if !self.find_devices() {
            log("waiting for input devices (power key / lid switch)");
        }
        if self.backlight.is_none() {
            if self.find_backlight() {
                log(&format!(
                    "using backlight {}",
                    self.backlight.as_deref().unwrap_or("")
                ));
            } else {
                log("no backlight device found, screen toggling disabled");
            }
        }
        if self.backlight.is_some() {
            self.screen_off = self.read_brightness().unwrap_or(0) == 0;
        }
        if let Some(path) = self.power_path.clone() {
            if self.power_fd.is_none() {
                match open_nonblocking(&path) {
                    Ok(fd) => {
                        self.power_fd = Some(fd);
                        log(&format!("watching power key {}", path));
                    }
                    Err(e) => {
                        log(&format!("cannot open power device {}: {}", path, e));
                        self.power_fd = None;
                    }
                }
            }
        }
        if let Some(path) = self.lid_path.clone() {
            if self.lid_fd.is_none() {
                match open_nonblocking(&path) {
                    Ok(fd) => {
                        self.lid_fd = Some(fd);
                        log(&format!("watching lid switch {}", path));
                    }
                    Err(e) => {
                        log(&format!("cannot open lid device {}: {}", path, e));
                        self.lid_fd = None;
                    }
                }
            }
        }
        if self.keyboard_fds.is_empty() {
            self.open_keyboard_devices();
        }
        if self.pointer_fds.is_empty() {
            self.open_pointer_devices();
        }
    }

    fn turn_off(&mut self) {
        if self.backlight.is_none() {
            return;
        }
        let brightness = self.read_brightness().unwrap_or(0);
        if self.screen_off && self.display_blanked && brightness == 0 {
            return; // already fully off
        }
        // Use the tracked user brightness; the current sysfs value may be
        // GNOME's idle-dim level and must not overwrite the real setting.
        let preferred = if self.user_brightness > 0 {
            self.user_brightness
        } else {
            best_user_brightness()
        };
        set_power_save(true);
        if preferred > 0 {
            save_state(preferred);
        }
        if let Err(e) = self.write_brightness(0) {
            log(&format!("failed to turn off backlight: {}", e));
            return;
        }
        self.screen_off = true;
        self.display_blanked = true;
        log("screen off");
    }

    fn start_suspend_timer(&mut self, delay_secs: f64, reason: &'static str) {
        if is_charging() {
            log(&format!(
                "suspend countdown skipped (charging; {} suspend control disabled)",
                reason
            ));
            return;
        }
        let deadline = now_secs() + delay_secs;
        self.suspend_deadline = Some(deadline);
        self.suspend_reason = Some(reason);
        log(&format!(
            "suspend countdown started ({}): suspend in {:.0} s",
            reason, delay_secs
        ));
    }

    fn cancel_suspend_timer(&mut self) {
        if self.suspend_deadline.is_some() {
            let reason = self.suspend_reason.unwrap_or("unknown");
            log(&format!("suspend countdown cancelled ({})", reason));
        }
        self.suspend_deadline = None;
        self.suspend_reason = None;
    }

    fn fire_suspend_timer(&mut self) {
        let reason = self.suspend_reason.take().unwrap_or("countdown");
        self.suspend_deadline = None;
        if is_charging() {
            log(&format!(
                "suspend countdown expired ({}) but system is charging; staying awake with screen off",
                reason
            ));
            return;
        }
        log(&format!(
            "suspend countdown expired ({}), suspending system",
            reason
        ));
        match run_cmd(&["systemctl", "suspend"], 0) {
            Some(res) if res.ok => {
                self.just_resumed = true;
                log("system resumed after countdown suspend");
            }
            Some(res) => log(&format!(
                "systemctl suspend failed: {}",
                res.stderr.trim()
            )),
            None => log("systemctl suspend could not be started"),
        }
    }

    fn turn_on(&mut self) {
        // Any screen-on request cancels a pending suspend countdown.
        self.cancel_suspend_timer();
        let t_turn_on = Instant::now();
        log("turn_on: start");
        if self.backlight.is_none() {
            return;
        }
        if !self.screen_off && !self.display_blanked && self.read_brightness().unwrap_or(0) > 0 {
            return; // already fully on
        }
        self.last_own_unblank = now_secs();
        let preferred = if self.user_brightness > 0 {
            self.user_brightness
        } else {
            best_user_brightness()
        };
        log(&format!(
            "turn_on: best_user_brightness {} ms",
            t_turn_on.elapsed().as_millis()
        ));
        let current = self.read_brightness().unwrap_or(0);
        if current > 0 {
            // Keep the panel dark while the display link re-initializes, so a
            // glitchy first frame is never shown with the backlight on.
            // Do not save here: this may be GNOME idle-dim's temporary value.
            if let Err(e) = self.write_brightness(0) {
                log(&format!("failed to dim backlight before wake: {}", e));
                return;
            }
        }
        let t_pw = Instant::now();
        // Start the kernel-failure window before unblanking: the Novatek
        // firmware load can fail while set_power_save(false) is still in
        // flight, and starting the window afterwards would miss it.
        let init_start = now_secs();
        let force_recovery = self.just_resumed;
        self.just_resumed = false;
        let mut pw_ok = false;
        if force_recovery {
            // After a system resume the panel can stay black even when the
            // firmware reports success. Instead of unblanking first and then
            // running a second full cycle, do exactly one complete blank →
            // unblank cycle (the manual "close lid, open lid" workaround).
            // The backlight is already 0, so nothing visible is delayed.
            log("wake after resume: one forced blank/unblank recovery cycle");
            set_power_save(true);
            thread::sleep(Duration::from_millis(RECOVERY_PANEL_OFF_MS));
        }
        // The compositor can still be busy right after resume; retry the
        // unblank a few times instead of trusting the first reply.
        for _ in 0..3 {
            pw_ok = set_power_save(false);
            if pw_ok {
                break;
            }
            thread::sleep(Duration::from_millis(400));
        }
        if !pw_ok {
            // A gdbus reply can be lost even though Mutter already processed
            // the request (e.g. a signal raced with the subprocess); trust
            // the real DPMS state in that case.
            self.update_display_state();
            if self.read_dpms().as_deref() == Some("On") {
                pw_ok = true;
            }
        }
        log(&format!(
            "turn_on: set_power_save(false) {} ms (ok={})",
            t_pw.elapsed().as_millis(),
            pw_ok
        ));
        if !pw_ok {
            // Keep the screen marked off so the next lid/power event (or the
            // resume handler) retries the whole wake sequence.
            log("compositor unblank failed; keeping screen off, will retry on next event");
            return;
        }
        let t_settle = Instant::now();
        thread::sleep(Duration::from_millis(DISPLAY_SETTLE_MS));
        let kernel_failed = kernel_init_failed_since(init_start);
        if force_recovery {
            if kernel_failed
                && !run_recovery_cycles(MAX_RECOVERY_CYCLES, "extra after forced cycle")
            {
                log(&format!(
                    "{} extra recovery cycles still reported failure; restoring brightness anyway",
                    MAX_RECOVERY_CYCLES
                ));
            }
        } else if kernel_failed
            && !run_recovery_cycles(MAX_RECOVERY_CYCLES, "kernel init failure")
        {
            log(&format!(
                "{} recovery cycles still reported failure; restoring brightness anyway",
                MAX_RECOVERY_CYCLES
            ));
        }
        log(&format!(
            "turn_on: settle+kernel_check {} ms",
            t_settle.elapsed().as_millis()
        ));
        let maximum = self.read_max_brightness();
        let mut value = preferred;
        if value <= 0 {
            value = load_state();
        }
        if value <= 0 || value > maximum {
            value = maximum;
        }
        if value > 20 {
            save_state(value);
        }
        if let Err(e) = self.write_brightness(value) {
            log(&format!("failed to turn on backlight: {}", e));
            return;
        }
        thread::sleep(Duration::from_millis(100));
        let actual = self.read_brightness().unwrap_or(-1);
        if actual != value {
            log(&format!(
                "brightness did not stick ({}), retrying once",
                actual
            ));
            if let Err(e) = self.write_brightness(value) {
                log(&format!("failed to retry backlight: {}", e));
            }
        }
        self.screen_on_at = now_secs();
        self.screen_off = false;
        self.display_blanked = false;
        log(&format!(
            "screen on (brightness {}) (turn_on total {} ms)",
            self.read_brightness().unwrap_or(-1),
            t_turn_on.elapsed().as_millis()
        ));
        // Touch/pen devices are re-added after the screen is on, so the udev
        // refresh never holds the backlight off; mice/touchpads are refreshed
        // right after in the same background worker.
        let touch_props = POINTER_PROPS_TOUCH.to_vec();
        let other_props = POINTER_PROPS_OTHER.to_vec();
        thread::spawn(move || {
            refresh_pointer_devices(&touch_props);
            refresh_pointer_devices(&other_props);
        });
        thread::sleep(Duration::from_millis(500));
        if self.read_dpms().as_deref() != Some("On") {
            log("compositor re-blanked the display, unblanking again");
            set_power_save(false);
            self.update_display_state();
        }
    }

    /// Current lid switch state from the kernel, falling back to the last
    /// observed SW_LID event if the ioctl is unavailable.
    fn lid_closed(&self) -> Option<bool> {
        if let Some(fd) = self.lid_fd {
            let mut sw = [0u8; 8];
            let ret = unsafe { ioctl(fd, EVIOCGSW, sw.as_mut_ptr() as *mut c_void) };
            if ret >= 0 {
                return Some(sw[0] & 1 != 0);
            }
        }
        self.last_lid.map(|v| v == 1)
    }

    fn on_resume(&mut self) {
        // After any suspend/wake, clear a still-pending countdown and resync.
        self.cancel_suspend_timer();
        // Force the recovery blank/unblank on the first screen-on after this
        // resume, regardless of which code path actually runs turn_on().
        self.just_resumed = true;
        // Read the real lid-switch state: after wake the kernel knows it even
        // if the SW_LID event is still queued or was delivered out of order.
        let lid_closed = self.lid_closed();
        if let Some(closed) = lid_closed {
            self.last_lid = Some(if closed { 1 } else { 0 });
        }
        if lid_closed == Some(false) {
            self.resuspend_count = 0;
            self.resuspend_window_start = 0.0;
        }
        if self.backlight.is_none() {
            return;
        }
        self.update_display_state();
        if lid_closed == Some(true) {
            // Closing the lid after a suspend must not light the screen:
            // blank it and go straight back to sleep, with loop protection.
            log("resume: lid is closed, keeping screen off");
            self.turn_off();
            if is_charging() {
                log("resume: lid closed but system is charging, keeping screen off (no re-suspend)");
                return;
            }
            let now = now_secs();
            if now - self.resuspend_window_start > RESUSPEND_WINDOW_SECS {
                self.resuspend_window_start = now;
                self.resuspend_count = 0;
            }
            self.resuspend_count += 1;
            if self.resuspend_count <= MAX_LID_RESUSPENDS {
                log(&format!(
                    "resume: lid-close wake, re-suspending (attempt {}/{})",
                    self.resuspend_count, MAX_LID_RESUSPENDS
                ));
                match run_cmd(&["systemctl", "suspend"], 0) {
                    Some(res) if res.ok => {
                        self.just_resumed = true;
                        log("system resumed after lid-close re-suspend");
                    }
                    Some(res) => log(&format!("re-suspend failed: {}", res.stderr.trim())),
                    None => log("re-suspend could not be started"),
                }
            } else {
                log("resume: lid-close wake loop detected, staying awake with screen off");
            }
            if self.lid_closed() == Some(true) {
                self.start_suspend_timer(LID_SUSPEND_DELAY_SECS, "lid close");
            }
            return;
        }
        if !self.screen_visibly_on() {
            self.screen_off = true;
            self.turn_on(); // also refreshes touch/pointer devices
        } else {
            self.screen_off = false;
            // The compositor may have already restored the display during
            // wake, so turn_on() was skipped; refresh input devices anyway,
            // otherwise the touchscreen mapping can stay stale.
            log("resume: display already on, refreshing input devices");
            let touch_props = POINTER_PROPS_TOUCH.to_vec();
            let other_props = POINTER_PROPS_OTHER.to_vec();
            thread::spawn(move || {
                refresh_pointer_devices(&touch_props);
                refresh_pointer_devices(&other_props);
            });
        }
        log("resume: backlight state resynced");
    }

    fn open_keyboard_devices(&mut self) {
        for path in find_keyboard_paths() {
            if self.keyboard_fds.iter().any(|(_, p)| *p == path) {
                continue;
            }
            match open_nonblocking(&path) {
                Ok(fd) => {
                    self.keyboard_fds.push((fd, path.clone()));
                    log(&format!("watching keyboard {}", path));
                }
                Err(e) => log(&format!("cannot open keyboard {}: {}", path, e)),
            }
        }
    }

    fn open_pointer_devices(&mut self) {
        for path in find_pointer_paths() {
            if self.pointer_fds.iter().any(|(_, p)| *p == path) {
                continue;
            }
            match open_nonblocking(&path) {
                Ok(fd) => {
                    self.pointer_fds.push((fd, path.clone()));
                    log(&format!("watching pointer {}", path));
                }
                Err(e) => log(&format!("cannot open pointer {}: {}", path, e)),
            }
        }
    }

    /// Reconcile watched keyboard/pointer devices with the current system:
    /// close fds for devices that disappeared and open newly connected ones
    /// (e.g. a Bluetooth keyboard/mouse that paired after boot).
    fn rescan_input_devices(&mut self) {
        // Only pay for the udevadm scans when the set of event nodes actually
        // changed; a plain directory read is far cheaper.
        let snapshot = input_events_snapshot();
        if snapshot == self.input_snapshot {
            return;
        }
        self.input_snapshot = snapshot;
        let keyboard_paths = find_keyboard_paths();
        self.keyboard_fds.retain(|(fd, path)| {
            if keyboard_paths.contains(path) {
                true
            } else {
                log(&format!("keyboard {} disappeared; closing fd", path));
                unsafe {
                    close(*fd);
                }
                false
            }
        });
        for path in keyboard_paths {
            if !self.keyboard_fds.iter().any(|(_, p)| *p == path) {
                match open_nonblocking(&path) {
                    Ok(fd) => {
                        self.keyboard_fds.push((fd, path.clone()));
                        log(&format!("watching keyboard {}", path));
                    }
                    Err(e) => log(&format!("cannot open keyboard {}: {}", path, e)),
                }
            }
        }
        let pointer_paths = find_pointer_paths();
        self.pointer_fds.retain(|(fd, path)| {
            if pointer_paths.contains(path) {
                true
            } else {
                log(&format!("pointer {} disappeared; closing fd", path));
                unsafe {
                    close(*fd);
                }
                false
            }
        });
        for path in pointer_paths {
            if !self.pointer_fds.iter().any(|(_, p)| *p == path) {
                match open_nonblocking(&path) {
                    Ok(fd) => {
                        self.pointer_fds.push((fd, path.clone()));
                        log(&format!("watching pointer {}", path));
                    }
                    Err(e) => log(&format!("cannot open pointer {}: {}", path, e)),
                }
            }
        }
    }

    /// Drain pending events from an input fd.
    /// Returns Ok(events) on success, Err(()) if the device disappeared.
    fn drain_events(&self, fd: RawFd) -> Result<Vec<InputEvent>, ()> {
        let mut events = Vec::new();
        let mut buf = [0u8; EVENT_SIZE];
        loop {
            let n = unsafe { read(fd, buf.as_mut_ptr() as *mut c_void, EVENT_SIZE) };
            if n < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == ErrorKind::WouldBlock {
                    break;
                }
                return Err(());
            }
            if n as usize != EVENT_SIZE {
                continue;
            }
            let ev = unsafe { ptr::read_unaligned(buf.as_ptr() as *const InputEvent) };
            events.push(ev);
        }
        Ok(events)
    }

    fn handle_power_lid_event(&mut self, fd: RawFd, is_power: bool) -> bool {
        let events = match self.drain_events(fd) {
            Ok(e) => e,
            Err(()) => {
                log(&format!("input device {} disappeared", fd));
                return false;
            }
        };
        for ev in events {
            if is_power {
                if ev.event_type == EV_KEY && ev.code == KEY_POWER && ev.value == 1 {
                    // A press queued while turn_on() was still running (slow
                    // panel recovery) must not toggle the screen back off.
                    let ev_time = ev.time_sec as f64 + ev.time_usec as f64 / 1_000_000.0;
                    if self.screen_visibly_on() && ev_time < self.screen_on_at - 0.2 {
                        log("power key ignored (stale press while screen was waking)");
                        continue;
                    }
                    let now = now_secs();
                    if now - self.last_toggle < 1.0 {
                        log("power key ignored (debounce)");
                        continue;
                    }
                    self.last_toggle = now;
                    log("power key pressed");
                    if self.screen_visibly_on() {
                        self.turn_off();
                        self.start_suspend_timer(POWER_SUSPEND_DELAY_SECS, "power button");
                    } else {
                        self.turn_on();
                    }
                }
            } else if ev.event_type == EV_SW && ev.code == SW_LID && Some(ev.value) != self.last_lid
            {
                self.last_lid = Some(ev.value);
                if ev.value == 1 {
                    log("lid closed");
                    if self.suspend_deadline.is_some() {
                        // The screen was already turned off (e.g. power button
                        // with its 15 s countdown); closing the lid should not
                        // restart the screen-off flow or extend the deadline.
                        log("lid closed while a suspend countdown is active, keeping existing countdown");
                    } else {
                        self.turn_off();
                        self.start_suspend_timer(LID_SUSPEND_DELAY_SECS, "lid close");
                    }
                } else if ev.value == 0 {
                    log("lid opened");
                    self.resuspend_count = 0;
                    self.resuspend_window_start = 0.0;
                    self.turn_on();
                }
            }
        }
        true
    }

    fn handle_keyboard_event(&mut self, fd: RawFd, path: &str) -> bool {
        let events = match self.drain_events(fd) {
            Ok(e) => e,
            Err(()) => {
                log(&format!("keyboard device {} disappeared", path));
                return false;
            }
        };
        if self.lid_closed() == Some(true) {
            return true; // lid closed: keyboard must not wake the screen
        }
        for ev in events {
            if ev.event_type == EV_KEY && ev.value == 1 && !self.screen_visibly_on() {
                log("keyboard wake");
                self.turn_on();
            }
        }
        true
    }

    fn handle_pointer_event(&mut self, fd: RawFd, path: &str) -> bool {
        let events = match self.drain_events(fd) {
            Ok(e) => e,
            Err(()) => {
                log(&format!("pointer device {} disappeared", path));
                return false;
            }
        };
        if self.lid_closed() == Some(true) {
            return true; // lid closed: pointer must not wake the screen
        }
        let mut wake = false;
        for ev in &events {
            if ev.event_type == EV_KEY && ev.value == 1 {
                wake = true;
                break;
            }
            if (ev.event_type == EV_REL || ev.event_type == EV_ABS) && ev.value != 0 {
                wake = true;
                break;
            }
        }
        if wake && !self.screen_visibly_on() {
            log(&format!("pointer wake ({})", path));
            self.turn_on();
        }
        true
    }

    fn shutdown(&mut self) {
        for fd in [self.power_fd.take(), self.lid_fd.take()]
            .into_iter()
            .flatten()
        {
            unsafe {
                close(fd);
            }
        }
        for (fd, _) in self.keyboard_fds.drain(..) {
            unsafe {
                close(fd);
            }
        }
        for (fd, _) in self.pointer_fds.drain(..) {
            unsafe {
                close(fd);
            }
        }
        log("daemon stopped");
    }
}

// ---------------------------------------------------------------------------
// Compositor / session interaction.
// ---------------------------------------------------------------------------

fn show_session(sid: &str) -> std::collections::HashMap<String, String> {
    let mut props = std::collections::HashMap::new();
    if let Some(res) = run_cmd(&["loginctl", "show-session", sid], 5) {
        for line in res.stdout.lines() {
            if let Some((k, v)) = line.split_once('=') {
                props.insert(k.to_string(), v.to_string());
            }
        }
    }
    props
}

fn graphical_session() -> Option<(String, u32)> {
    let out = run_cmd(&["loginctl", "list-sessions", "--no-legend"], 5)?;
    for line in out.stdout.lines() {
        let sid = line.split_whitespace().next()?;
        let info = show_session(sid);
        if info.get("Class").map(String::as_str) != Some("user") {
            continue;
        }
        if info.get("Active").map(String::as_str) != Some("yes") {
            continue;
        }
        if !matches!(info.get("Type").map(String::as_str), Some("wayland") | Some("x11")) {
            continue;
        }
        let uid: u32 = info.get("User")?.parse().ok()?;
        let name = info.get("Name")?;
        if !name.is_empty() && uid > 0 {
            return Some((name.clone(), uid));
        }
        return None;
    }
    None
}

fn set_power_save(on: bool) -> bool {
    let t0 = Instant::now();
    let Some((username, uid)) = graphical_session() else {
        log(&format!(
            "set_power_save: no graphical session ({} ms)",
            t0.elapsed().as_millis()
        ));
        return false;
    };
    log(&format!(
        "set_power_save: graphical_session {} ms",
        t0.elapsed().as_millis()
    ));
    let cmd = [
        "gdbus",
        "call",
        "--session",
        "--dest",
        "org.gnome.Mutter.DisplayConfig",
        "--object-path",
        "/org/gnome/Mutter/DisplayConfig",
        "--method",
        "org.freedesktop.DBus.Properties.Set",
        "org.gnome.Mutter.DisplayConfig",
        "PowerSaveMode",
        if on { "<1>" } else { "<0>" },
    ];
    match run_cmd_as_user(&cmd, &username, uid, 5) {
        Some(res) if res.ok => {
            log(&format!(
                "set_power_save: gdbus call {} ms",
                t0.elapsed().as_millis()
            ));
            true
        }
        Some(res) => {
            log(&format!(
                "PowerSaveMode({}) failed: {}",
                on,
                res.stderr.trim()
            ));
            false
        }
        None => {
            log(&format!("PowerSaveMode({}) error", on));
            false
        }
    }
}

/// Read GNOME's idle-dim settings and return the expected dimmed brightness
/// on the sysfs scale (min + (max - min) * percent / 100). Returns None when
/// idle-dim is disabled or the settings cannot be read.
fn idle_dim_brightness(max: i64) -> Option<i64> {
    let (username, uid) = graphical_session()?;
    let enabled = run_cmd_as_user(
        &[
            "gsettings",
            "get",
            "org.gnome.settings-daemon.plugins.power",
            "idle-dim",
        ],
        &username,
        uid,
        5,
    )?;
    if !enabled.stdout.contains("true") {
        return None;
    }
    let pct = run_cmd_as_user(
        &[
            "gsettings",
            "get",
            "org.gnome.settings-daemon.plugins.power",
            "idle-brightness",
        ],
        &username,
        uid,
        5,
    )?;
    let pct: i64 = pct.stdout.split_whitespace().last()?.parse().ok()?;
    if pct <= 0 || max <= BACKLIGHT_MIN {
        return None;
    }
    Some(BACKLIGHT_MIN + (max - BACKLIGHT_MIN) * pct / 100)
}

/// Parse the current backlight serial and connector name from Mutter's
/// Backlight property, e.g. `(uint32 78, [... 'connector': <'DSI-1'> ...])`.
fn compositor_backlight_target(username: &str, uid: u32) -> Option<(u32, String)> {
    let res = run_cmd_as_user(
        &[
            "gdbus",
            "call",
            "--session",
            "--dest",
            "org.gnome.Mutter.DisplayConfig",
            "--object-path",
            "/org/gnome/Mutter/DisplayConfig",
            "--method",
            "org.freedesktop.DBus.Properties.Get",
            "org.gnome.Mutter.DisplayConfig",
            "Backlight",
        ],
        username,
        uid,
        5,
    )?;
    if !res.ok {
        return None;
    }
    let serial = parse_after(&res.stdout, "uint32 ")?;
    let connector = parse_connector(&res.stdout)?;
    Some((serial, connector))
}

fn parse_connector(s: &str) -> Option<String> {
    let needle = "'connector': <'";
    let start = s.find(needle)? + needle.len();
    let end = s[start..].find("'>")? + start;
    Some(s[start..end].to_string())
}

/// Set the backlight through Mutter so its Backlight property (and therefore
/// the GNOME brightness setting) stays in sync with the real value. Retries
/// once with a fresh serial if the first call reports a stale serial.
fn set_compositor_backlight(value: i64) -> bool {
    for _ in 0..2 {
        let Some((username, uid)) = graphical_session() else {
            return false;
        };
        let Some((serial, connector)) = compositor_backlight_target(&username, uid) else {
            return false;
        };
        let serial_str = serial.to_string();
        let connector_arg = format!("'{}'", connector);
        let value_str = value.to_string();
        let res = run_cmd_as_user(
            &[
                "gdbus",
                "call",
                "--session",
                "--dest",
                "org.gnome.Mutter.DisplayConfig",
                "--object-path",
                "/org/gnome/Mutter/DisplayConfig",
                "--method",
                "org.gnome.Mutter.DisplayConfig.SetBacklight",
                &serial_str,
                &connector_arg,
                &value_str,
            ],
            &username,
            uid,
            5,
        );
        if let Some(r) = res {
            if r.ok {
                return true;
            }
        }
    }
    false
}

fn get_compositor_backlight_info() -> Option<(u32, u32)> {
    let t0 = Instant::now();
    let (username, uid) = graphical_session()?;
    log(&format!(
        "get_backlight_info: graphical_session {} ms",
        t0.elapsed().as_millis()
    ));
    let cmd = [
        "gdbus",
        "call",
        "--session",
        "--dest",
        "org.gnome.Mutter.DisplayConfig",
        "--object-path",
        "/org/gnome/Mutter/DisplayConfig",
        "--method",
        "org.freedesktop.DBus.Properties.Get",
        "org.gnome.Mutter.DisplayConfig",
        "Backlight",
    ];
    let res = run_cmd_as_user(&cmd, &username, uid, 5)?;
    log(&format!(
        "get_backlight_info: gdbus call {} ms",
        t0.elapsed().as_millis()
    ));
    if !res.ok {
        return None;
    }
    let serial = parse_after(&res.stdout, "uint32 ")?;
    let value = parse_after(&res.stdout, "'value': <")?;
    if value > 0 {
        Some((serial, value))
    } else {
        None
    }
}

fn parse_after(s: &str, needle: &str) -> Option<u32> {
    let idx = s.find(needle)?;
    let digits: String = s[idx + needle.len()..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn get_compositor_brightness() -> Option<i64> {
    get_compositor_backlight_info().map(|(_, v)| v as i64)
}

fn best_user_brightness() -> i64 {
    let prop = get_compositor_brightness();
    let state = load_state();
    if let Some(p) = prop {
        if p > 20 {
            return p;
        }
    }
    if state > 20 {
        return state;
    }
    if let Some(p) = prop {
        if p > 0 {
            return p;
        }
    }
    state
}

// ---------------------------------------------------------------------------
// State file.
// ---------------------------------------------------------------------------

fn save_state(value: i64) {
    let result = (|| -> io::Result<()> {
        fs::create_dir_all(STATE_DIR)?;
        let tmp = format!("{}.tmp", STATE_FILE);
        fs::write(&tmp, format!("{}\n", value))?;
        fs::rename(&tmp, STATE_FILE)
    })();
    if let Err(e) = result {
        log(&format!("cannot save state: {}", e));
    }
}

fn load_state() -> i64 {
    fs::read_to_string(STATE_FILE)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(-1)
}

/// True while the device is on external power / charging. While charging,
/// the daemon only turns the screen off and never suspends the system.
fn is_charging() -> bool {
    for name in sorted_dir_names("/sys/class/power_supply") {
        let base = format!("/sys/class/power_supply/{}", name);
        let supply_type = match fs::read_to_string(format!("{}/type", base)) {
            Ok(s) => s.trim().to_string(),
            Err(_) => continue,
        };
        let status = fs::read_to_string(format!("{}/status", base))
            .ok()
            .map(|s| s.trim().to_string());
        let online = fs::read_to_string(format!("{}/online", base))
            .ok()
            .map(|s| s.trim().to_string());
        let on_power = if supply_type == "Battery" {
            matches!(status.as_deref(), Some("Charging") | Some("Full"))
        } else {
            online.as_deref() == Some("1")
                || matches!(status.as_deref(), Some("Charging") | Some("Full"))
        };
        if on_power {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Input device refresh and kernel failure detection.
// ---------------------------------------------------------------------------

fn find_pointer_events(props: &[&str]) -> Vec<String> {
    let mut events = Vec::new();
    for name in sorted_dir_names("/sys/class/input") {
        if !name.starts_with("event") {
            continue;
        }
        let device = format!("/dev/input/{}", name);
        let Some(res) = run_cmd(&["udevadm", "info", "-q", "property", "-n", &device], 5) else {
            continue;
        };
        let matched = res.stdout.lines().any(|line| {
            if let Some((k, v)) = line.split_once('=') {
                props.contains(&k) && v == "1"
            } else {
                false
            }
        });
        if matched {
            events.push(name);
        }
    }
    events
}

fn refresh_pointer_devices(props: &[&str]) {
    let t0 = Instant::now();
    let events = find_pointer_events(props);
    if events.is_empty() {
        return;
    }
    log(&format!("refreshing {} input device(s)", events.len()));
    for evname in events {
        for action in ["remove", "add"] {
            let _ = run_cmd(
                &[
                    "udevadm",
                    "trigger",
                    &format!("--action={}", action),
                    "--sysname",
                    &evname,
                ],
                5,
            );
            thread::sleep(Duration::from_millis(50));
        }
    }
    let _ = run_cmd(&["udevadm", "settle", "--timeout=3"], 10);
    log(&format!(
        "input device refresh done in {} ms",
        t0.elapsed().as_millis()
    ));
}

fn input_events_snapshot() -> String {
    sorted_dir_names("/sys/class/input")
        .into_iter()
        .filter(|n| n.starts_with("event"))
        .collect::<Vec<_>>()
        .join(",")
}

fn find_keyboard_paths() -> Vec<String> {
    let mut paths = Vec::new();
    for name in sorted_dir_names("/sys/class/input") {
        if !name.starts_with("event") {
            continue;
        }
        let device = format!("/dev/input/{}", name);
        let Some(res) = run_cmd(&["udevadm", "info", "-q", "property", "-n", &device], 5) else {
            continue;
        };
        let is_keyboard = res.stdout.lines().any(|line| {
            if let Some((k, v)) = line.split_once('=') {
                k == "ID_INPUT_KEYBOARD" && v == "1"
            } else {
                false
            }
        });
        if is_keyboard {
            paths.push(device);
        }
    }
    paths
}

fn find_pointer_paths() -> Vec<String> {
    find_pointer_events(&POINTER_PROPS_OTHER)
        .into_iter()
        .map(|name| format!("/dev/input/{}", name))
        .collect()
}

fn kernel_init_failed_since(epoch: f64) -> bool {
    let since = format_epoch(epoch);
    let Some(res) = run_cmd(
        &[
            "journalctl",
            "-k",
            "--since",
            &since,
            "--no-pager",
            "-o",
            "short",
        ],
        5,
    ) else {
        return false;
    };
    KERNEL_INIT_FAILURE_MARKERS
        .iter()
        .any(|marker| res.stdout.contains(marker))
}

/// Run up to `max_cycles` blank → unblank recovery cycles, checking the
/// kernel log after each one. Returns true as soon as a cycle shows no init
/// failure.
fn run_recovery_cycles(max_cycles: u32, reason: &str) -> bool {
    for attempt in 1..=max_cycles {
        log(&format!(
            "display/touch recovery blank/unblank {}/{} ({})",
            attempt, max_cycles, reason
        ));
        set_power_save(true);
        thread::sleep(Duration::from_millis(RECOVERY_PANEL_OFF_MS));
        let cycle_start = now_secs();
        set_power_save(false);
        thread::sleep(Duration::from_millis(DISPLAY_SETTLE_MS));
        if !kernel_init_failed_since(cycle_start) {
            log("display/touch init OK after recovery cycle");
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Main loop.
// ---------------------------------------------------------------------------

fn main() {
    // Registered at startup so an early SIGUSR1 (e.g. the resume hook firing
    // right after boot) can never kill the daemon with the default action.
    unsafe {
        signal(SIGUSR1, on_sigusr1 as *const () as usize);
        signal(SIGTERM, on_sigterm as *const () as usize);
        signal(SIGINT, on_sigint as *const () as usize);
    }

    let mut daemon = Daemon::new();
    log("screen-toggle daemon starting");

    let mut last_blanked: Option<bool> = None;
    while RUNNING.load(Ordering::SeqCst) {
        if SIGUSR1_FLAG.swap(false, Ordering::SeqCst) {
            daemon.on_resume();
        }
        if SIGTERM_FLAG.load(Ordering::SeqCst) || SIGINT_FLAG.load(Ordering::SeqCst) {
            break;
        }

        // Fire the suspend countdown if the deadline passed without the
        // system suspending (or being cancelled by a screen-on event).
        if daemon.suspend_deadline.is_some() && is_charging() {
            log("charging detected, suspend countdown cancelled");
            daemon.cancel_suspend_timer();
        }
        if daemon
            .suspend_deadline
            .map_or(false, |deadline| now_secs() >= deadline)
        {
            daemon.fire_suspend_timer();
        }

        daemon.update_display_state();
        if !daemon.display_blanked && !daemon.screen_off {
            daemon.track_user_brightness();
        }
        let blanked = daemon.display_blanked;
        if let Some(prev) = last_blanked {
            if prev && !blanked && now_secs() - daemon.last_own_unblank > 3.0 {
                if daemon.lid_closed() == Some(true) {
                    // The compositor may unblank the display during wake even
                    // though the lid is closed; keep it off instead of
                    // treating this as a user screen-on request.
                    log("external unblank ignored (lid closed); keeping screen off");
                    daemon.turn_off();
                } else if daemon.screen_off {
                    log("display unblanked externally; turning screen on");
                    daemon.turn_on();
                } else {
                    log("display unblanked externally; refreshing input devices");
                    let touch_props = POINTER_PROPS_TOUCH.to_vec();
                    let other_props = POINTER_PROPS_OTHER.to_vec();
                    thread::spawn(move || {
                        refresh_pointer_devices(&touch_props);
                        refresh_pointer_devices(&other_props);
                    });
                }
            }
        }
        last_blanked = Some(blanked);

        if daemon.power_fd.is_none()
            || daemon.lid_fd.is_none()
            || daemon.backlight.is_none()
            || daemon.keyboard_fds.is_empty()
            || daemon.pointer_fds.is_empty()
        {
            daemon.setup();
        }

        if now_secs() - daemon.last_input_rescan > INPUT_RESCAN_INTERVAL_SECS {
            daemon.rescan_input_devices();
            daemon.last_input_rescan = now_secs();
        }

        let mut pollfds: Vec<PollFd> = Vec::new();
        if let Some(fd) = daemon.power_fd {
            pollfds.push(PollFd {
                fd,
                events: POLLIN,
                revents: 0,
            });
        }
        if let Some(fd) = daemon.lid_fd {
            pollfds.push(PollFd {
                fd,
                events: POLLIN,
                revents: 0,
            });
        }
        for (fd, _) in &daemon.keyboard_fds {
            pollfds.push(PollFd {
                fd: *fd,
                events: POLLIN,
                revents: 0,
            });
        }
        for (fd, _) in &daemon.pointer_fds {
            pollfds.push(PollFd {
                fd: *fd,
                events: POLLIN,
                revents: 0,
            });
        }
        if pollfds.is_empty() {
            thread::sleep(Duration::from_millis(500));
            continue;
        }

        let ready = unsafe { poll(pollfds.as_mut_ptr(), pollfds.len(), 500) };
        if ready <= 0 {
            continue;
        }
        for pfd in &pollfds {
            if pfd.revents & (POLLIN | POLLERR | POLLHUP) == 0 {
                continue;
            }
            if Some(pfd.fd) == daemon.power_fd {
                if !daemon.handle_power_lid_event(pfd.fd, true) {
                    if let Some(fd) = daemon.power_fd.take() {
                        unsafe {
                            close(fd);
                        }
                    }
                }
            } else if Some(pfd.fd) == daemon.lid_fd {
                if !daemon.handle_power_lid_event(pfd.fd, false) {
                    if let Some(fd) = daemon.lid_fd.take() {
                        unsafe {
                            close(fd);
                        }
                    }
                }
            } else if let Some(path) = daemon
                .keyboard_fds
                .iter()
                .find(|(fd, _)| *fd == pfd.fd)
                .map(|(_, p)| p.clone())
            {
                if !daemon.handle_keyboard_event(pfd.fd, &path) {
                    daemon.keyboard_fds.retain(|(fd, _)| *fd != pfd.fd);
                    unsafe {
                        close(pfd.fd);
                    }
                }
            } else if let Some(path) = daemon
                .pointer_fds
                .iter()
                .find(|(fd, _)| *fd == pfd.fd)
                .map(|(_, p)| p.clone())
            {
                if !daemon.handle_pointer_event(pfd.fd, &path) {
                    daemon.pointer_fds.retain(|(fd, _)| *fd != pfd.fd);
                    unsafe {
                        close(pfd.fd);
                    }
                }
            }
        }
    }
    daemon.shutdown();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_cmd_completes_normally() {
        let res = run_cmd(&["echo", "hi"], 5).expect("command should run");
        assert!(res.ok);
        assert_eq!(res.stdout.trim(), "hi");
    }

    #[test]
    fn run_cmd_timeout_kills_hung_command() {
        let t0 = Instant::now();
        let res = run_cmd(&["sleep", "10"], 1).expect("command should run");
        assert!(!res.ok, "timed-out command must report failure");
        assert!(
            t0.elapsed().as_secs() < 3,
            "timeout should fire before the command finishes"
        );
        assert!(res.stderr.contains("timed out"), "stderr: {}", res.stderr);
    }

    #[test]
    fn parses_compositor_backlight_target() {
        let s = "(<(uint32 78, [{'connector': <'DSI-1'>, 'active': <true>, \
                  'min': <20>, 'max': <2047>, 'value': <2047>}])>,)";
        assert_eq!(parse_after(s, "uint32 "), Some(78));
        assert_eq!(parse_connector(s).as_deref(), Some("DSI-1"));
    }
}
