use std::time::{Duration, Instant};

const STROKE: i32 = 52;
const MAX_VERTICAL: i32 = 34;
const MAX_LEG: Duration = Duration::from_millis(300);
const MAX_GESTURE: Duration = Duration::from_millis(720);

pub struct Shake {
    extreme_x: i32,
    origin_y: i32,
    direction: i32,
    legs: u8,
    started: Instant,
    last_leg: Instant,
}

impl Shake {
    pub fn new(x: i32, y: i32, now: Instant) -> Self {
        Self {
            extreme_x: x,
            origin_y: y,
            direction: 0,
            legs: 0,
            started: now,
            last_leg: now,
        }
    }

    /// Returns the direction of the final stroke once per drag.
    pub fn observe(&mut self, x: i32, y: i32, now: Instant) -> Option<i32> {
        if self.legs == 3 {
            return None;
        }
        if (y - self.origin_y).abs() > MAX_VERTICAL
            || now.saturating_duration_since(self.last_leg) > MAX_LEG
            || now.saturating_duration_since(self.started) > MAX_GESTURE
        {
            *self = Self::new(x, y, now);
            return None;
        }
        let dx = x - self.extreme_x;
        if self.direction != 0 && dx.signum() == self.direction {
            self.extreme_x = x;
            return None;
        }
        if dx.abs() < STROKE {
            return None;
        }
        self.extreme_x = x;
        self.direction = dx.signum();
        self.last_leg = now;
        self.legs += 1;
        (self.legs == 3).then_some(self.direction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_alternating_strokes_trigger_once() {
        let now = Instant::now();
        let mut shake = Shake::new(100, 200, now);
        assert_eq!(
            shake.observe(175, 202, now + Duration::from_millis(90)),
            None
        );
        assert_eq!(
            shake.observe(80, 198, now + Duration::from_millis(225)),
            None
        );
        assert_eq!(
            shake.observe(165, 206, now + Duration::from_millis(370)),
            Some(1)
        );
        assert_eq!(
            shake.observe(90, 200, now + Duration::from_millis(510)),
            None
        );
    }

    #[test]
    fn small_corrections_slow_strokes_and_vertical_drags_do_not_trigger() {
        let now = Instant::now();
        for (points, delay) in [
            ([(180, 200), (165, 200), (230, 200), (220, 200)], 80),
            ([(175, 200), (80, 200), (170, 200), (75, 200)], 360),
            ([(175, 280), (80, 360), (170, 440), (75, 520)], 95),
        ] {
            let mut shake = Shake::new(100, 200, now);
            for (i, (x, y)) in points.into_iter().enumerate() {
                assert_eq!(
                    shake.observe(x, y, now + Duration::from_millis((i as u64 + 1) * delay)),
                    None
                );
            }
        }
    }
}
