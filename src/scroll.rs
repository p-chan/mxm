//! Software scrolling: `mxm daemon` diverts the wheels to HID++ and posts the scroll events itself,
//! the way Logi Options+ does, so that its scroll speed settings mean the same in mxm.
//!
//! The steps and constants below were matched to recordings of Options+ (the HID++ input and the
//! scroll events it posted, on an MX Master 4 over Bluetooth); see PROTOCOL.md. Posting events
//! needs the Accessibility permission.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

/// Inches the pointer-scroll model assigns to one notch of the main wheel
const INCHES_PER_NOTCH: f64 = 7.0 / 54.0;
/// Screen resolution of the model (pixels per inch)
const DPI: f64 = 100.0;
/// Interval of the thumb wheel scroll timer
const THUMB_PERIOD: Duration = Duration::from_micros(16_600);
/// Thumb wheel ticks without input before the scrolling slows down
const THUMB_IDLE_TICKS: u32 = 7;

/// Software scrolling settings of the main wheel
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WheelScroll {
    /// 0-100 (the "Scrolling speed" percentage of Options+); 50 is the default
    pub speed: u8,
    /// High-resolution reports (true) or one report per notch (false)
    pub smooth: bool,
}

/// Software scrolling settings of the thumb wheel
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThumbScroll {
    /// 0-100 (the thumb wheel "Speed" percentage of Options+); 50 is the default
    pub speed: u8,
}

/// State of the main wheel needed to turn its reports into scrolling
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WheelState {
    pub scroll: WheelScroll,
    /// Reported units per notch (1 without high resolution)
    pub multiplier: u8,
    /// The wheel spins freely (no ratchet)
    pub freewheel: bool,
}

pub enum Input {
    /// HiResWheel movement: `delta` per period, for `periods` periods
    Wheel {
        delta: i16,
        periods: u8,
        at: Instant,
        state: WheelState,
    },
    /// ThumbWheel movement and rotation status (0 inactive, 1 start, 2 active, 3 stop)
    Thumb {
        rotation: i16,
        status: u8,
        at: Instant,
        scroll: ThumbScroll,
    },
}

/// Movement to post on one axis, in the direction the device reported
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    pub pixels: f64,
    /// Distance the wheel moved, for the line deltas
    pub inches: f64,
    /// First event of a thumb wheel gesture
    pub begin: bool,
}

/// Posts scroll events from a background thread
pub struct Scroller {
    tx: Sender<Input>,
}

impl Scroller {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || Animator::default().run(&rx));
        Self { tx }
    }

    pub fn send(&self, input: Input) {
        let _ = self.tx.send(input);
    }
}

// ---- Main wheel ----

/// Ignores the first units after the wheel changes direction (and after it rests), so that a
/// slight backlash does not scroll back
#[derive(Debug, Default)]
struct Hysteresis {
    /// Units still ignored when the wheel turns negative / positive
    negative: i32,
    positive: i32,
    last: Option<Instant>,
}

impl Hysteresis {
    fn filter(&mut self, delta: i32, at: Instant, multiplier: u8, freewheel: bool) -> i32 {
        if delta == 0 || multiplier < 4 {
            return delta;
        }
        let (rest, cap) = if freewheel && multiplier > 7 {
            (4, 4)
        } else {
            (1, 1)
        };
        if let Some(last) = self.last {
            let idle = at.saturating_duration_since(last).as_secs_f64();
            if idle >= 1.0 {
                let steps = idle as i32;
                for v in [&mut self.negative, &mut self.positive] {
                    *v = if *v >= rest {
                        (*v - steps).max(rest)
                    } else {
                        (*v + steps).min(rest)
                    };
                }
            }
        }
        self.last = Some(at);
        if delta > 0 {
            self.positive = (self.positive + delta).min(cap);
            let take = self.negative.min(delta);
            self.negative -= take;
            delta - take
        } else {
            self.negative = (self.negative - delta).min(cap);
            let take = self.positive.min(-delta);
            self.positive -= take;
            delta + take
        }
    }
}

/// Rounds the wheel movement to whole notches (for the line deltas in ratchet mode)
#[derive(Debug, Default)]
struct RatchetSteps {
    multiplier: i32,
    remainder: i32,
}

impl RatchetSteps {
    fn set_multiplier(&mut self, multiplier: u8) {
        let m = i32::from(multiplier);
        if self.multiplier != m {
            self.multiplier = m;
            self.remainder = 0;
        }
    }

    fn steps(&mut self, delta: i32) -> i32 {
        let m = self.multiplier;
        if m <= 1 {
            return delta;
        }
        let half = m / 2;
        let total = self.remainder + delta;
        self.remainder = if total < 0 {
            (total - half) % m + half
        } else {
            (total + half - 1) % m - (half - 1)
        };
        total - self.remainder
    }
}

/// Main wheel scrolling: one event per report period, accelerated by the smoothed wheel speed
#[derive(Debug, Default)]
pub struct MainWheel {
    last: Option<Instant>,
    /// Smoothed finger speed (inches per second)
    smoothed: f64,
    hysteresis: Hysteresis,
    ratchet: RatchetSteps,
}

impl MainWheel {
    pub fn input(&mut self, delta: i16, periods: u8, at: Instant, st: WheelState) -> Vec<Motion> {
        let periods = periods.max(1);
        let inches_per_unit = INCHES_PER_NOTCH / f64::from(st.multiplier.max(1));
        let speed = speed_setting(st.scroll.speed);
        self.ratchet.set_multiplier(st.multiplier);
        let elapsed = self
            .last
            .map(|l| at.saturating_duration_since(l).as_secs_f64());
        match elapsed {
            None => self.ratchet.remainder = 0,
            Some(e) if e >= 2.0 => self.ratchet.remainder = 0,
            Some(_) => {}
        }
        let delta = i32::from(delta);
        let mut out = Vec::with_capacity(usize::from(periods));
        for _ in 0..periods {
            // Time of one period, kept within 8-100 ms; a pause of 100 ms or more starts over
            let period = match elapsed.map(|e| e / f64::from(periods)) {
                None => 0.1,
                Some(p) if p < 0.008 => 0.008,
                Some(p) if p < 0.1 => p,
                Some(_) => {
                    self.smoothed = 0.0;
                    0.1
                }
            };
            let steps = self.ratchet.steps(delta);
            let moved = if st.freewheel { delta } else { steps };
            let filtered = self
                .hysteresis
                .filter(delta, at, st.multiplier, st.freewheel);
            let finger = inches_per_unit * f64::from(filtered) / period;
            self.smoothed = 0.95 * self.smoothed + 0.05 * finger.abs();
            out.push(Motion {
                pixels: finger * wheel_gain(self.smoothed, speed) * period * DPI,
                inches: (inches_per_unit * f64::from(moved)).abs(),
                begin: false,
            });
        }
        self.last = Some(at);
        out
    }
}

/// Speed setting of the acceleration curves: 0.06 at 0%, 1.2 at 100%
fn speed_setting(speed: u8) -> f64 {
    0.06 + (1.2 - 0.06) * f64::from(speed.min(100)) / 100.0
}

/// Linear interpolation between curve points; `beyond` past the last one
fn interpolate(x: f64, xs: &[f64], ys: &[f64], beyond: f64) -> f64 {
    for i in 1..xs.len() {
        if x < xs[i] {
            return ys[i - 1] + (x - xs[i - 1]) / (xs[i] - xs[i - 1]) * (ys[i] - ys[i - 1]);
        }
    }
    beyond
}

/// Acceleration of the main wheel by the smoothed speed (inches per second)
fn wheel_gain(smoothed: f64, speed: f64) -> f64 {
    let y = |base: f64| 1.0 + (base - 1.0) * speed;
    interpolate(
        smoothed,
        &[0.0, 0.3, 0.6, 60.0, 120.0],
        &[y(1.0), y(1.0), y(12.0), y(12.0), y(6.0)],
        // Past the curve Options+ takes a sentinel (1 - 2s), which would scroll backwards; keep
        // the last point instead (out of reach in practice)
        y(6.0),
    )
}

// ---- Thumb wheel ----

/// Thumb wheel scrolling: while the wheel turns, a timer repeats the last movement every 16.6 ms,
/// and slows down after the wheel stops
#[derive(Debug, Default)]
pub struct ThumbWheel {
    active: bool,
    next_tick: Option<Instant>,
    /// Last rotation (held between reports)
    rotation: i16,
    multiplier: f64,
    /// 1 while the wheel turns, decays after it stops
    level: f64,
    idle_ticks: u32,
    ending: bool,
    smoothed: f64,
}

impl ThumbWheel {
    /// Starts or continues a gesture; returns the first event of a new gesture
    pub fn input(
        &mut self,
        rotation: i16,
        status: u8,
        at: Instant,
        scroll: ThumbScroll,
    ) -> Option<Motion> {
        let stopping = status == 0 || status == 3;
        if rotation == 0 {
            if stopping && self.active {
                self.ending = true;
            }
            return None;
        }
        // Speed 0-100 maps to 0.5x-8x
        self.multiplier = 2f64.powf(4.0 * f64::from(scroll.speed.min(100)) / 100.0 - 1.0);
        self.rotation = rotation;
        if self.active {
            self.level = if self.level >= 0.99 {
                1.0
            } else {
                self.level + (1.0 - self.level) * 0.3
            };
            self.idle_ticks = 0;
            // The status of the newest report counts (a new start cancels a pending stop)
            self.ending = stopping;
            return None;
        }
        self.active = true;
        self.level = 1.0;
        self.idle_ticks = 0;
        self.ending = stopping;
        self.smoothed = 0.0;
        self.next_tick = Some(at + THUMB_PERIOD);
        Some(self.emit(true, false))
    }

    pub fn next_tick(&self) -> Option<Instant> {
        self.next_tick.filter(|_| self.active)
    }

    pub fn tick(&mut self, now: Instant) -> Option<Motion> {
        if !self.active {
            return None;
        }
        let next = self.next_tick.map_or(now, |t| t + THUMB_PERIOD);
        self.next_tick = Some(if next <= now {
            now + THUMB_PERIOD
        } else {
            next
        });
        if self.ending {
            self.active = false;
            return Some(self.emit(false, true));
        }
        self.idle_ticks += 1;
        if self.idle_ticks >= THUMB_IDLE_TICKS {
            self.level *= 0.15;
            if self.level <= 0.01 {
                self.active = false;
                return None;
            }
        }
        Some(self.emit(false, false))
    }

    fn emit(&mut self, begin: bool, end: bool) -> Motion {
        let period = THUMB_PERIOD.as_secs_f64();
        let mut scaled = self.multiplier * f64::from(self.rotation);
        if end {
            scaled = scaled.trunc();
        }
        let v = scaled * self.level / (360.0 * period);
        self.smoothed = 0.99 * self.smoothed + 0.01 * v;
        if self.smoothed.abs() < v.abs() {
            self.smoothed = v;
        }
        Motion {
            pixels: thumb_gain(self.smoothed.abs()) * v * DPI * period,
            inches: (v * period).abs(),
            begin,
        }
    }
}

/// Acceleration of the thumb wheel by the smoothed speed (inches per second)
fn thumb_gain(smoothed: f64) -> f64 {
    interpolate(
        smoothed,
        &[0.0, 0.3, 20.0, 100.0],
        &[4.0, 8.0, 30.0, 4.0],
        4.0,
    )
}

// ---- Events ----

/// Values posted for one axis
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Posted {
    pub pixels: i32,
    pub lines: i32,
    /// Lines in 16.16 fixed point
    pub fixed: i32,
}

/// Turns the movement of one axis into event values, carrying fractions of a pixel forward
#[derive(Debug, Default)]
struct Injector {
    remainder: f64,
    last_dir: f64,
    pixels: f64,
    inches: f64,
}

impl Injector {
    /// `pixels` is signed in the posted direction; `None` when nothing is posted
    fn push(&mut self, pixels: f64, inches: f64, begin: bool) -> Option<Posted> {
        if begin {
            self.remainder = 0.0;
            self.last_dir = 0.0;
        }
        self.remainder += pixels;
        let whole = self.remainder.trunc();
        self.remainder -= whole;
        if whole == 0.0 {
            return None;
        }
        let dir = if pixels < 0.0 { -1.0 } else { 1.0 };
        if self.last_dir != dir {
            self.pixels = 0.0;
            self.inches = 0.0;
        }
        self.last_dir = dir;
        self.pixels += pixels;
        self.inches += inches;
        // Line deltas follow once the wheel moved far enough
        let lines = if inches >= 0.02 || self.inches > 0.06 {
            let l = self.pixels / 10.0;
            self.pixels = 0.0;
            self.inches = 0.0;
            l
        } else {
            0.0
        };
        let whole_lines = if lines.trunc() == 0.0 && lines != 0.0 {
            dir
        } else {
            lines.trunc()
        };
        Some(Posted {
            pixels: whole as i32,
            lines: whole_lines as i32,
            fixed: (lines.round() * 65536.0) as i32,
        })
    }
}

#[derive(Default)]
struct Animator {
    wheel: MainWheel,
    thumb: ThumbWheel,
    vertical: Injector,
    horizontal: Injector,
}

impl Animator {
    fn run(&mut self, rx: &Receiver<Input>) {
        loop {
            let input = match self.thumb.next_tick() {
                Some(t) => match rx.recv_timeout(t.saturating_duration_since(Instant::now())) {
                    Ok(i) => Some(i),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                },
                None => match rx.recv() {
                    Ok(i) => Some(i),
                    Err(_) => return,
                },
            };
            match input {
                Some(Input::Wheel {
                    delta,
                    periods,
                    at,
                    state,
                }) => {
                    for m in self.wheel.input(delta, periods, at, state) {
                        self.post_vertical(m);
                    }
                }
                Some(Input::Thumb {
                    rotation,
                    status,
                    at,
                    scroll,
                }) => {
                    if let Some(m) = self.thumb.input(rotation, status, at, scroll) {
                        self.post_horizontal(m);
                    }
                }
                None => {}
            }
            // Also when inputs keep arriving, so that they do not hold back the timer
            if self.thumb.next_tick().is_some_and(|t| t <= Instant::now())
                && let Some(m) = self.thumb.tick(Instant::now())
            {
                self.post_horizontal(m);
            }
        }
    }

    // Posted events scroll opposite to the reported movement, as with Options+; the direction is
    // set on the device (`invert`)
    fn post_vertical(&mut self, m: Motion) {
        if let Some(p) = self.vertical.push(-m.pixels, m.inches, m.begin) {
            platform::post_scroll(p, Posted::default());
        }
    }

    fn post_horizontal(&mut self, m: Motion) {
        if let Some(p) = self.horizontal.push(-m.pixels, m.inches, m.begin) {
            platform::post_scroll(Posted::default(), p);
        }
    }
}

/// Whether this process may post events (Accessibility). Asks the user when `prompt` is set
pub fn has_permission(prompt: bool) -> bool {
    platform::has_permission(prompt)
}

pub const SUPPORTED: bool = platform::SUPPORTED;

/// Whether the main display is asleep (also while the Mac is going to sleep)
pub fn display_asleep() -> bool {
    platform::display_asleep()
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::c_void;

    use super::Posted;

    pub const SUPPORTED: bool = true;

    type CFTypeRef = *const c_void;

    const SCROLL_UNIT_PIXEL: u32 = 0;
    const HID_EVENT_TAP: u32 = 0;
    const DELTA_AXIS_1: u32 = 11;
    const DELTA_AXIS_2: u32 = 12;
    const FIXED_DELTA_AXIS_1: u32 = 93;
    const FIXED_DELTA_AXIS_2: u32 = 94;
    const POINT_DELTA_AXIS_1: u32 = 96;
    const POINT_DELTA_AXIS_2: u32 = 97;
    const SCROLL_PHASE: u32 = 99;
    const MOMENTUM_PHASE: u32 = 123;
    /// `NSEvent.isDirectionInvertedFromDevice`
    const DIRECTION_INVERTED: u32 = 137;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreateScrollWheelEvent2(
            source: *const c_void,
            units: u32,
            wheel_count: u32,
            wheel1: i32,
            wheel2: i32,
            wheel3: i32,
        ) -> CFTypeRef;
        fn CGEventSetIntegerValueField(event: CFTypeRef, field: u32, value: i64);
        fn CGEventPost(tap: u32, event: CFTypeRef);
        fn CGPreflightPostEventAccess() -> bool;
        fn CGRequestPostEventAccess() -> bool;
        fn CGMainDisplayID() -> u32;
        fn CGDisplayIsAsleep(display: u32) -> u32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: CFTypeRef);
    }

    /// Posts a continuous (pixel) scroll at the pointer. Positive: content moves down / right
    pub fn post_scroll(vertical: Posted, horizontal: Posted) {
        unsafe {
            let e = CGEventCreateScrollWheelEvent2(
                std::ptr::null(),
                SCROLL_UNIT_PIXEL,
                2,
                vertical.pixels,
                horizontal.pixels,
                0,
            );
            if e.is_null() {
                return;
            }
            for (field, value) in [
                (DELTA_AXIS_1, vertical.lines),
                (DELTA_AXIS_2, horizontal.lines),
                (FIXED_DELTA_AXIS_1, vertical.fixed),
                (FIXED_DELTA_AXIS_2, horizontal.fixed),
                (POINT_DELTA_AXIS_1, vertical.pixels),
                (POINT_DELTA_AXIS_2, horizontal.pixels),
                (SCROLL_PHASE, 0),
                (MOMENTUM_PHASE, 0),
                (DIRECTION_INVERTED, 0),
            ] {
                CGEventSetIntegerValueField(e, field, i64::from(value));
            }
            CGEventPost(HID_EVENT_TAP, e);
            CFRelease(e);
        }
    }

    pub fn display_asleep() -> bool {
        unsafe { CGDisplayIsAsleep(CGMainDisplayID()) != 0 }
    }

    pub fn has_permission(prompt: bool) -> bool {
        unsafe { CGPreflightPostEventAccess() || (prompt && CGRequestPostEventAccess()) }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::Posted;

    pub const SUPPORTED: bool = false;

    pub fn post_scroll(_vertical: Posted, _horizontal: Posted) {}

    pub fn has_permission(_prompt: bool) -> bool {
        false
    }

    pub fn display_asleep() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(speed: u8) -> WheelState {
        WheelState {
            scroll: WheelScroll {
                speed,
                smooth: true,
            },
            multiplier: 15,
            freewheel: false,
        }
    }

    fn ms(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    #[test]
    fn speed_setting_matches_options_plus() {
        assert!((speed_setting(0) - 0.06).abs() < 1e-9);
        assert!((speed_setting(50) - 0.63).abs() < 1e-9);
        assert!((speed_setting(100) - 1.2).abs() < 1e-9);
    }

    #[test]
    fn slow_scrolling_is_about_0_86_pixels_per_unit() {
        let mut w = MainWheel::default();
        let t0 = Instant::now();
        // Two units every 50 ms, after the direction hysteresis took its unit
        w.input(2, 1, t0, state(85));
        let m = w.input(2, 1, ms(t0, 50), state(85));
        assert!((m[0].pixels - 2.0 * 0.8642).abs() < 0.01, "{m:?}");
    }

    #[test]
    fn fast_scrolling_reaches_the_plateau_of_the_speed() {
        for (speed, plateau) in [(0, 1.435), (50, 6.853), (85, 10.646), (100, 12.272)] {
            let mut w = MainWheel::default();
            let t0 = Instant::now();
            let mut last = 0.0;
            for i in 0..40 {
                last = w.input(5, 1, ms(t0, 8 * i), state(speed))[0].pixels / 5.0;
            }
            assert!((last - plateau).abs() < 0.01, "{speed}%: {last}");
        }
    }

    #[test]
    fn a_pause_of_100_ms_starts_over() {
        let mut w = MainWheel::default();
        let t0 = Instant::now();
        for i in 0..40 {
            w.input(5, 1, ms(t0, 8 * i), state(100));
        }
        let m = w.input(5, 1, ms(t0, 8 * 39 + 100), state(100));
        assert!((m[0].pixels / 5.0 - 0.8642).abs() < 0.01, "{m:?}");
    }

    #[test]
    fn the_first_unit_after_a_reversal_is_ignored() {
        let mut h = Hysteresis::default();
        let t0 = Instant::now();
        assert_eq!(h.filter(3, t0, 15, false), 3);
        assert_eq!(h.filter(-2, ms(t0, 10), 15, false), -1);
        assert_eq!(h.filter(-2, ms(t0, 20), 15, false), -2);
        // Low resolution has no hysteresis
        assert_eq!(h.filter(1, ms(t0, 30), 1, false), 1);
    }

    #[test]
    fn periods_repeat_the_delta() {
        let mut w = MainWheel::default();
        let t0 = Instant::now();
        w.input(3, 1, t0, state(50));
        assert_eq!(w.input(3, 2, ms(t0, 30), state(50)).len(), 2);
    }

    #[test]
    fn thumb_wheel_repeats_the_last_rotation_every_tick() {
        let mut t = ThumbWheel::default();
        let t0 = Instant::now();
        let scroll = ThumbScroll { speed: 100 };
        let first = t.input(1, 1, t0, scroll).unwrap();
        // 8x speed, 1 unit: about 20.4 px per tick (as Options+ at 100%)
        assert!((first.pixels - 20.4).abs() < 0.1, "{first:?}");
        assert!(first.begin);
        let tick = t.tick(t.next_tick().unwrap()).unwrap();
        assert!((tick.pixels - first.pixels).abs() < 0.01);
        // Slows down after 7 idle ticks, then stops
        let mut n = 1;
        while let Some(at) = t.next_tick() {
            if t.tick(at).is_none() {
                break;
            }
            n += 1;
        }
        assert!((8..=10).contains(&n), "{n} ticks");
    }

    #[test]
    fn thumb_wheel_stops_on_the_stop_status() {
        let mut t = ThumbWheel::default();
        let t0 = Instant::now();
        let scroll = ThumbScroll { speed: 100 };
        t.input(1, 1, t0, scroll);
        assert!(t.input(0, 3, ms(t0, 5), scroll).is_none());
        assert!(t.tick(t.next_tick().unwrap()).is_some());
        assert_eq!(t.next_tick(), None);
    }

    #[test]
    fn injector_carries_fractions_and_reports_lines() {
        let mut i = Injector::default();
        assert_eq!(i.push(0.6, 0.0, false), None);
        let p = i.push(0.6, 0.0, false).unwrap();
        assert_eq!((p.pixels, p.lines, p.fixed), (1, 0, 0));
        // A large movement reports the accumulated pixels as lines
        let p = i.push(-49.0, 0.13, false).unwrap();
        assert_eq!((p.pixels, p.lines, p.fixed), (-48, -4, -5 * 65536));
    }

    #[test]
    fn ratchet_steps_round_to_whole_notches() {
        let mut r = RatchetSteps::default();
        r.set_multiplier(15);
        assert_eq!(r.steps(4), 0);
        assert_eq!(r.steps(4), 0);
        // 12 units are closer to the next notch: it counts, and 3 units are owed
        assert_eq!(r.steps(4), 15);
        assert_eq!(r.steps(3), 0);
    }

    #[test]
    fn the_gain_stays_positive_past_the_curve() {
        let s = speed_setting(50);
        assert_eq!(wheel_gain(500.0, s), 1.0 + 5.0 * s);
    }

    #[test]
    fn a_new_start_cancels_a_pending_stop() {
        let mut t = ThumbWheel::default();
        let t0 = Instant::now();
        let scroll = ThumbScroll { speed: 100 };
        t.input(1, 1, t0, scroll);
        t.input(0, 3, ms(t0, 5), scroll);
        t.input(1, 1, ms(t0, 10), scroll);
        t.tick(t.next_tick().unwrap());
        assert!(t.next_tick().is_some());
    }
}
