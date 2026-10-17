use glam::{Mat4, Vec3};

const PITCH_LIMIT: f32 = 89.0_f32.to_radians();

#[derive(Debug, Clone)]
pub struct OrbitCamera {
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub min_distance: f32,
    pub max_distance: f32,
}

impl OrbitCamera {
    pub fn new(target: Vec3, distance: f32) -> Self {
        let distance = distance.max(0.2);
        Self {
            target,
            yaw: 0.0,
            pitch: 0.55,
            distance,
            min_distance: 0.05,
            max_distance: 100.0,
        }
    }

    pub fn orbit(&mut self, delta_x: f32, delta_y: f32) {
        if delta_x.is_finite() {
            self.yaw = (self.yaw - delta_x * 0.008).rem_euclid(std::f32::consts::TAU);
        }
        if delta_y.is_finite() {
            self.pitch = (self.pitch + delta_y * 0.008).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        }
    }

    pub fn zoom(&mut self, scroll_delta: f32) {
        let scroll_delta = if scroll_delta.is_finite() {
            scroll_delta.clamp(-100.0, 100.0)
        } else {
            0.0
        };
        self.distance = (self.distance * (-scroll_delta * 0.12).exp())
            .clamp(self.min_distance, self.max_distance);
    }

    pub fn view_projection(&self, aspect_ratio: f32) -> Mat4 {
        let horizontal = self.pitch.cos();
        let direction = Vec3::new(
            horizontal * self.yaw.sin(),
            self.pitch.sin(),
            horizontal * self.yaw.cos(),
        );
        let eye = self.target + direction * self.distance;
        let view = Mat4::look_at_rh(eye, self.target, Vec3::Y);
        let projection =
            Mat4::perspective_rh(45.0_f32.to_radians(), aspect_ratio.max(0.01), 0.01, 1000.0);
        projection * view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orbit_clamps_pitch_and_zoom_clamps_distance() {
        let mut camera = OrbitCamera::new(Vec3::ZERO, 2.0);
        camera.orbit(0.0, 100_000.0);
        assert!(camera.pitch <= PITCH_LIMIT);

        camera.zoom(100_000.0);
        assert_eq!(camera.distance, camera.min_distance);
        camera.zoom(-100_000.0);
        assert_eq!(camera.distance, camera.max_distance);
    }

    #[test]
    fn view_projection_is_finite_for_normal_viewports() {
        let camera = OrbitCamera::new(Vec3::ZERO, 3.0);
        assert!(camera
            .view_projection(16.0 / 9.0)
            .to_cols_array()
            .iter()
            .all(|value| value.is_finite()));
    }

    #[test]
    fn non_finite_input_does_not_corrupt_camera() {
        let mut camera = OrbitCamera::new(Vec3::ZERO, 3.0);
        camera.orbit(f32::INFINITY, f32::NAN);
        camera.zoom(f32::NAN);
        assert!(camera
            .view_projection(1.0)
            .to_cols_array()
            .iter()
            .all(|v| v.is_finite()));
    }
}
