//! Pose math for changing the tracking reference frame consistently.
use openxr::{Posef, Quaternionf, Vector3f};

pub fn rotate(q: Quaternionf, v: Vector3f) -> Vector3f {
    let t = Vector3f { x: 2.0*(q.y*v.z-q.z*v.y), y: 2.0*(q.z*v.x-q.x*v.z), z: 2.0*(q.x*v.y-q.y*v.x) };
    Vector3f { x: v.x+q.w*t.x+q.y*t.z-q.z*t.y, y: v.y+q.w*t.y+q.z*t.x-q.x*t.z, z: v.z+q.w*t.z+q.x*t.y-q.y*t.x }
}
pub fn compose(a: Posef, b: Posef) -> Posef {
    let p = rotate(a.orientation, b.position);
    let (q, r) = (a.orientation, b.orientation);
    Posef {
        orientation: Quaternionf {
            x: q.w*r.x+q.x*r.w+q.y*r.z-q.z*r.y,
            y: q.w*r.y-q.x*r.z+q.y*r.w+q.z*r.x,
            z: q.w*r.z+q.x*r.y-q.y*r.x+q.z*r.w,
            w: q.w*r.w-q.x*r.x-q.y*r.y-q.z*r.z,
        },
        position: Vector3f { x: a.position.x+p.x, y: a.position.y+p.y, z: a.position.z+p.z },
    }
}
pub fn recentered_origin(head: Posef, floor_level: bool) -> Result<Posef, &'static str> {
    // Project the forward direction onto the horizontal plane. Do not bake
    // headset pitch or roll into the tracking frame.
    let forward = rotate(head.orientation, Vector3f { x: 0.0, y: 0.0, z: -1.0 });
    if forward.x*forward.x+forward.z*forward.z < 1e-6 { return Err("look forward before recentering"); }
    let half_yaw = (-forward.x).atan2(-forward.z)*0.5;
    Ok(Posef {
        orientation: Quaternionf { x: 0.0, y: half_yaw.sin(), z: 0.0, w: half_yaw.cos() },
        position: Vector3f { y: if floor_level { 0.0 } else { head.position.y }, ..head.position },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inverse(p: Posef) -> Posef {
        let q = Quaternionf { x: -p.orientation.x, y: -p.orientation.y, z: -p.orientation.z, w: p.orientation.w };
        Posef { orientation: q, position: rotate(q, Vector3f { x: -p.position.x, y: -p.position.y, z: -p.position.z }) }
    }
    fn near(a: f32, b: f32) { assert!((a-b).abs()<1e-5, "{a} != {b}"); }
    #[test]
    fn eye_origin_removes_starting_height_but_floor_preserves_it() {
        for height in [1.1, 1.75] { // Seated and standing.
            let head = Posef { position: Vector3f { x: 2.0, y: height, z: -3.0 }, ..Posef::IDENTITY };
            for floor in [false, true] {
                let relative = compose(inverse(recentered_origin(head, floor).unwrap()), head);
                near(relative.position.x, 0.0); near(relative.position.z, 0.0);
                near(relative.position.y, if floor { height } else { 0.0 });
            }
        }
    }
    #[test]
    fn recenter_rotates_head_hands_and_velocity_together() {
        let q = Quaternionf { y: (core::f32::consts::FRAC_PI_4).sin(), w: (core::f32::consts::FRAC_PI_4).cos(), x: 0.0, z: 0.0 };
        let head = Posef { orientation: q, position: Vector3f { x: 2.0, y: 1.6, z: 3.0 } };
        let hand_offset = Posef { position: Vector3f { x: 0.3, y: -0.4, z: -0.5 }, ..Posef::IDENTITY };
        let hand = compose(head, hand_offset);
        let change = inverse(recentered_origin(head, false).unwrap());
        let new_hand = compose(change, hand);
        near(compose(change, head).orientation.w, 1.0);
        near(new_hand.position.x, 0.3); near(new_hand.position.y, -0.4); near(new_hand.position.z, -0.5);
        let velocity = rotate(change.orientation, rotate(q, hand_offset.position));
        near(velocity.x, 0.3); near(velocity.y, -0.4); near(velocity.z, -0.5);
    }
    #[test]
    fn recenter_keeps_pitch_out_of_origin() {
        let pitched = Posef { orientation: Quaternionf { x: 0.3_f32.sin(), w: 0.3_f32.cos(), y: 0.0, z: 0.0 }, ..Posef::IDENTITY };
        let origin = recentered_origin(pitched, false).unwrap();
        near(origin.orientation.x, 0.0); near(origin.orientation.w, 1.0);
        near(compose(inverse(origin), pitched).orientation.x, pitched.orientation.x);
    }
}
