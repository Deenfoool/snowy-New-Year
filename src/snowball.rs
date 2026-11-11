//! Snowball material and three-ball snowman stacking.
//!
//! Balls belong to the desktop session after leaving a window snowcap.
//! They no longer depend on the lifetime of their source HWND.

pub const MAX_BALLS: usize = 24;
pub const MAX_RADIUS: f32 = 36.0;
pub const MIN_BALL_AREA: f32 = 75.0;
pub const GRAVITY: f32 = 850.0;

#[derive(Clone, Copy, Debug)]
pub struct Snowball {
    pub id: u64,
    pub x: f32,
    pub y: f32,
    /// 2D representation of the collected snow, in square screen pixels.
    pub area: f32,
    pub vx: f32,
    pub vy: f32,
    pub parent: Option<u64>,
    pub grounded: bool,
}

impl Snowball {
    pub fn new(id: u64, x: f32, y: f32) -> Self {
        Self {
            id,
            x,
            y,
            area: 0.0,
            vx: 0.0,
            vy: 0.0,
            parent: None,
            grounded: false,
        }
    }

    pub fn radius(self) -> f32 {
        (self.area / std::f32::consts::PI).sqrt().clamp(3.5, MAX_RADIUS)
    }

    pub fn collect(&mut self, area: f32) {
        if area.is_finite() && area > 0.0 {
            self.area = (self.area + area)
                .min(std::f32::consts::PI * MAX_RADIUS * MAX_RADIUS);
        }
    }

    pub fn is_formed(self) -> bool {
        self.area >= MIN_BALL_AREA
    }
}

/// Move a ball together with all balls stacked on top of it.
pub fn move_with_children(balls: &mut [Snowball], id: u64, x: f32, y: f32) {
    let Some(index) = balls.iter().position(|ball| ball.id == id) else {
        return;
    };
    let (dx, dy) = (x - balls[index].x, y - balls[index].y);
    balls[index].x = x;
    balls[index].y = y;
    let all_descendants: Vec<u64> = balls
        .iter()
        .filter(|ball| is_descendant(balls, ball.id, id))
        .map(|ball| ball.id)
        .collect();
    for ball in balls.iter_mut() {
        if all_descendants.contains(&ball.id) {
            ball.x += dx;
            ball.y += dy;
        }
    }
}

fn is_descendant(balls: &[Snowball], candidate: u64, ancestor: u64) -> bool {
    let mut current = balls.iter().find(|ball| ball.id == candidate).and_then(|b| b.parent);
    for _ in 0..3 {
        let Some(id) = current else { return false };
        if id == ancestor { return true }
        current = balls.iter().find(|ball| ball.id == id).and_then(|b| b.parent);
    }
    false
}

fn chain_height(balls: &[Snowball], id: u64) -> usize {
    let mut height = 1;
    let mut current = id;
    for _ in 0..3 {
        let Some(parent) = balls.iter().find(|b| b.id == current).and_then(|b| b.parent) else {
            break;
        };
        current = parent;
        height += 1;
    }
    height
}

fn subtree_height(balls: &[Snowball], id: u64) -> usize {
    let Some(child) = balls.iter().find(|b| b.parent == Some(id)) else {
        return 1;
    };
    1 + subtree_height(balls, child.id)
}

/// Snap a dragged ball onto a smaller stack if it is near the top of a larger one.
/// Only a single child is allowed for each ball, and stacks never exceed 3 balls.
/// A grounded ball may become the base of a snowman.
pub fn try_stack(balls: &mut [Snowball], moving_id: u64) -> bool {
    let Some(moving) = balls.iter().find(|b| b.id == moving_id).copied() else {
        return false;
    };
    if !moving.is_formed() {
        return false;
    }
    let mut best: Option<(u64, f32, f32, f32)> = None;
    for parent in balls.iter() {
        if parent.id == moving_id || !parent.is_formed() {
            continue;
        }
        if !parent.grounded && parent.parent.is_none() {
            continue;
        }
        if is_descendant(balls, parent.id, moving_id) {
            continue;
        }
        if balls.iter().any(|b| b.parent == Some(parent.id) && b.id != moving_id) {
            continue;
        }
        if chain_height(balls, parent.id) + subtree_height(balls, moving_id) > 3 {
            continue;
        }
        let child_radius = moving.radius();
        let parent_radius = parent.radius();
        if child_radius >= parent_radius * 0.94 {
            continue;
        }
        let target_x = parent.x;
        let target_y = parent.y - 0.92 * (parent_radius + child_radius);
        let dx = (moving.x - target_x).abs();
        let dy = (moving.y - target_y).abs();
        if dx > (parent_radius * 0.85).max(18.0)
            || dy > (parent_radius + child_radius) * 0.78 + 12.0
        {
            continue;
        }
        let distance = dx * dx + dy * dy;
        if best.is_none_or(|b| distance < b.3) {
            best = Some((parent.id, target_x, target_y, distance));
        }
    }
    let Some((parent_id, target_x, target_y, _)) = best else {
        return false;
    };
    if let Some(ball) = balls.iter_mut().find(|b| b.id == moving_id) {
        ball.parent = Some(parent_id);
        ball.grounded = false;
        ball.vx = 0.0;
        ball.vy = 0.0;
    }
    move_with_children(balls, moving_id, target_x, target_y);
    true
}

/// Three balls form a finished snowman if the bottom one rests on the ground.
pub fn is_head_of_snowman(balls: &[Snowball], id: u64) -> bool {
    let Some(head) = balls.iter().find(|b| b.id == id) else {
        return false;
    };
    let Some(middle) = head.parent.and_then(|p| balls.iter().find(|b| b.id == p)) else {
        return false;
    };
    let Some(base) = middle.parent.and_then(|p| balls.iter().find(|b| b.id == p)) else {
        return false;
    };
    base.grounded && base.parent.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn made(id: u64, x: f32, y: f32, radius: f32, grounded: bool) -> Snowball {
        Snowball {
            area: std::f32::consts::PI * radius * radius,
            grounded,
            ..Snowball::new(id, x, y)
        }
    }

    #[test]
    fn a_snowman_needs_three_smaller_balls() {
        let mut balls = vec![
            made(1, 300.0, 500.0, 30.0, true),
            made(2, 302.0, 454.0, 23.0, false),
            made(3, 298.0, 413.0, 15.0, false),
        ];
        assert!(try_stack(&mut balls, 2));
        assert!(try_stack(&mut balls, 3));
        assert!(is_head_of_snowman(&balls, 3));
        assert_eq!(balls[2].parent, Some(2));
    }

    #[test]
    fn cannot_stack_large_on_small_or_build_four_high() {
        let mut balls = vec![
            made(1, 100.0, 500.0, 22.0, true),
            made(2, 100.0, 460.0, 27.0, false),
        ];
        assert!(!try_stack(&mut balls, 2));
        balls[1].area = std::f32::consts::PI * 16.0 * 16.0;
        assert!(try_stack(&mut balls, 2));
        balls.push(made(3, 100.0, 428.0, 10.0, false));
        assert!(try_stack(&mut balls, 3));
        balls.push(made(4, 100.0, 406.0, 7.0, false));
        assert!(!try_stack(&mut balls, 4));
    }

    #[test]
    fn moving_base_moves_the_whole_snowman() {
        let mut balls = vec![
            made(1, 300.0, 500.0, 30.0, true),
            made(2, 300.0, 451.0, 22.0, false),
            made(3, 300.0, 418.0, 15.0, false),
        ];
        balls[1].parent = Some(1);
        balls[2].parent = Some(2);
        move_with_children(&mut balls, 1, 340.0, 495.0);
        assert_eq!((balls[1].x, balls[1].y), (340.0, 446.0));
        assert_eq!((balls[2].x, balls[2].y), (340.0, 413.0));
    }
}
