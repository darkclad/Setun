//! TPIC: the ternary programmable interrupt controller (ISA §8.2).
//!
//! 81 (3^4) IRQ lines; a lower number is a higher priority. A raised line
//! stays pending until the handler `CLAIM`s it; it is then in service until
//! `EOI`. A line interrupts only if no line of equal or higher priority is in
//! service.

pub const LINES: usize = 81;

pub struct Pic {
    vecbase: i64,
    masked: [bool; LINES],
    pending: [bool; LINES],
    in_service: [bool; LINES],
    select: i64,
}

impl Default for Pic {
    fn default() -> Self {
        Self::new()
    }
}

impl Pic {
    pub fn new() -> Self {
        Pic {
            vecbase: 1,
            masked: [true; LINES],
            pending: [false; LINES],
            in_service: [false; LINES],
            select: 0,
        }
    }

    pub fn raise(&mut self, irq: usize) {
        self.pending[irq] = true;
    }

    /// The line that would interrupt now, if any.
    fn best(&self) -> Option<usize> {
        let ceiling = self.in_service.iter().position(|&s| s).unwrap_or(LINES);
        (0..ceiling).find(|&i| self.pending[i] && !self.masked[i])
    }

    pub fn pending_vector(&self) -> Option<i64> {
        self.best().map(|i| self.vecbase + i as i64)
    }

    fn line(v: i64) -> Option<usize> {
        (0..LINES as i64).contains(&v).then_some(v as usize)
    }

    pub fn read(&mut self, r: i64) -> i64 {
        match r {
            0 => self.vecbase,
            1 => match self.best() {
                Some(i) => {
                    self.pending[i] = false;
                    self.in_service[i] = true;
                    i as i64
                }
                None => -1,
            },
            5 => self.select,
            6 => match Self::line(self.select) {
                Some(i) if self.masked[i] => -1,
                Some(i) if self.pending[i] => 1,
                _ => 0,
            },
            _ => 0,
        }
    }

    pub fn write(&mut self, r: i64, v: i64) {
        match (r, Self::line(v)) {
            (0, _) => self.vecbase = v,
            (2, Some(i)) => self.in_service[i] = false,
            (3, Some(i)) => self.masked[i] = false,
            (4, Some(i)) => self.masked[i] = true,
            (5, _) => self.select = v,
            _ => {}
        }
    }
}
