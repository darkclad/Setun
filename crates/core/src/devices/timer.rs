//! Interval timer and real-time clock (ISA §8.2).

/// Interval timer on a 1 MHz time base.
/// `CTRL`: −1 one-shot, 0 stopped, +1 periodic.
#[derive(Default)]
pub struct Timer {
    ctrl: i64,
    period: i64,
    count: i64,
}

impl Timer {
    /// Advance by `us` microseconds; true if the timer fired at least once.
    pub fn advance(&mut self, us: i64) -> bool {
        if self.ctrl == 0 || self.period <= 0 {
            return false;
        }
        self.count -= us;
        if self.count > 0 {
            return false;
        }
        if self.ctrl == 1 {
            // Periodic: keep phase; several missed periods fire once.
            self.count = self.period - (-self.count) % self.period;
        } else {
            self.ctrl = 0;
            self.count = 0;
        }
        true
    }

    /// Microseconds of advance until the timer next fires, if it is running.
    pub fn us_until_fire(&self) -> Option<i64> {
        (self.ctrl != 0 && self.period > 0).then(|| self.count.max(1))
    }

    pub fn read(&self, r: i64, now_us: i64) -> i64 {
        match r {
            0 => self.ctrl,
            1 => self.period,
            2 => self.count,
            3 => now_us,
            _ => 0,
        }
    }

    pub fn write(&mut self, r: i64, v: i64) {
        match r {
            0 => {
                self.ctrl = v.signum();
                self.count = self.period;
            }
            1 => self.period = v.max(0),
            2 => self.count = v.max(0),
            _ => {}
        }
    }
}

/// Real-time clock: Unix time plus an alarm. Setting `SECONDS` also resets
/// the sub-second phase, like a real RTC's divider.
pub struct Rtc {
    /// Unix time in microseconds at emulated time 0.
    base_us: i64,
    alarm: i64,
}

impl Rtc {
    pub fn new(unix_time_s: i64) -> Self {
        Rtc { base_us: unix_time_s * 1_000_000, alarm: 0 }
    }

    fn unix_us(&self, now_us: i64) -> i64 {
        self.base_us + now_us
    }

    /// True (once) when a set alarm time has been reached.
    /// Microseconds of advance until the alarm fires, if one is set.
    pub fn us_until_alarm(&self, now_us: i64) -> Option<i64> {
        (self.alarm != 0).then(|| (self.alarm * 1_000_000 - self.unix_us(now_us)).max(1))
    }

    pub fn check_alarm(&mut self, now_us: i64) -> bool {
        if self.alarm != 0 && self.unix_us(now_us).div_euclid(1_000_000) >= self.alarm {
            self.alarm = 0;
            return true;
        }
        false
    }

    pub fn read(&self, r: i64, now_us: i64) -> i64 {
        match r {
            0 => self.unix_us(now_us).div_euclid(1_000_000),
            1 => self.unix_us(now_us).rem_euclid(1_000_000),
            2 => self.alarm,
            _ => 0,
        }
    }

    pub fn write(&mut self, r: i64, v: i64, now_us: i64) {
        match r {
            0 => self.base_us = v * 1_000_000 - now_us,
            2 => self.alarm = v,
            _ => {}
        }
    }
}
