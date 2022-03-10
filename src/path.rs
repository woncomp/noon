use bevy_ecs::prelude::*;
use lyon::{
    builder::{Build, WithSvg},
    iterator::PathIterator,
    PathEvent,
};
use nannou::lyon::{
    algorithms::length::approximate_length,
    geom::{point, LineSegment},
    lyon_algorithms::walk::RepeatedPattern,
    path::traits::{PathBuilder, SvgPathBuilder},
};

use nannou::lyon::{lyon_algorithms::walk::walk_along_path, path as lyon};

use crate::{Interpolate, PathCompletion, Point, Position, Size};

#[derive(Debug, Clone, Component)]
pub struct Path(pub(crate) lyon::Path);

impl Path {
    pub fn svg_builder() -> WithSvg<lyon::path::Builder> {
        lyon::Path::svg_builder()
    }
    pub fn builder() -> lyon::path::Builder {
        lyon::path::Builder::new()
    }
}

impl Interpolate for Path {
    fn interp(&self, other: &Self, progress: f32) -> Self {
        let tol = 0.5;
        let progress = progress.min(1.0).max(0.0);

        if progress <= 0.001 {
            self.clone()
        } else if progress >= 0.999 {
            other.clone()
        } else {
            // 1. Calculate the length of path1 and path2
            // 2. Iterate through path2, to construct length ratio vector
            // 3. Walk through path1, and insert line segments that map to path2
            // 4. Do step 2 for path 1
            // 5. Do step 3 for path 2
            // 6. Now we should have same number of lines (Assuming continuous shape)
            // 7. Interpolate line points from path 1 to path 2

            let get_line_lengths = |path: &Path| {
                path.0
                    .iter()
                    .flattened(tol)
                    .filter(|e| matches!(e, PathEvent::Line { .. }))
                    .scan(0.0, |d, event| {
                        match event {
                            PathEvent::Line { from, to } => {
                                *d += (to - from).length();
                            }
                            _ => (),
                        };
                        Some(*d)
                    })
                    .collect::<Vec<f32>>()
            };

            let path1_lengths = get_line_lengths(self);
            let path2_lengths = get_line_lengths(other);

            let len_1 = path1_lengths.last().unwrap();
            let len_2 = path2_lengths.last().unwrap();

            let ratios = combine_vectors_with_ordering(&path1_lengths, &path2_lengths);

            let lengths_1: Vec<f32> = ratios
                .iter()
                .zip(ratios.iter().skip(1))
                .map(|(a, b)| b - a)
                .map(|val| val * len_1)
                .collect();
            let lengths_2: Vec<f32> = ratios
                .iter()
                .zip(ratios.iter().skip(1))
                .map(|(a, b)| b - a)
                .map(|val| val * len_2)
                .collect();

            let mut p1 = Vec::new();
            let mut p2 = Vec::new();

            let mut pattern_1 = RepeatedPattern {
                callback: &mut |position, _t, d| {
                    p1.push(position);
                    true
                },
                intervals: &lengths_1,
                index: 0,
            };
            let mut pattern_2 = RepeatedPattern {
                callback: &mut |position, _t, d| {
                    p2.push(position);
                    true
                },
                intervals: &lengths_2,
                index: 0,
            };

            walk_along_path(self.0.iter().flattened(tol), 0.0, &mut pattern_1);
            walk_along_path(other.0.iter().flattened(tol), 0.0, &mut pattern_2);

            let mut builder = Path::svg_builder();
            p1.iter().zip(p2.iter()).for_each(|(&p1, p2)| {
                builder.line_to(p1.interp(p2, progress));
            });
            builder.close();

            Path(builder.build())
        }
    }
}

// Combine two vectors which are both monotonically increasing by normalized ordering
fn combine_vectors_with_ordering(v1: &[f32], v2: &[f32]) -> Vec<f32> {
    let mut combined = Vec::new();

    let s1 = *v1.last().unwrap();
    let s2 = *v2.last().unwrap();

    let mut v2_iter = v2.iter().peekable();
    for val1 in v1.into_iter() {
        while let Some(val2) = v2_iter.peek() {
            if **val2 / s2 < val1 / s1 {
                combined.push(**val2 / s2);
                v2_iter.next();
            } else {
                break;
            }
        }
        combined.push(val1 / s1);
    }

    combined
}

pub trait PathComponent {
    fn path(size: &Size) -> Path;
}

pub trait MeasureLength {
    fn approximate_length(&self, tolerance: f32) -> f32;
}

impl MeasureLength for Path {
    fn approximate_length(&self, tolerance: f32) -> f32 {
        approximate_length(self.0.iter(), tolerance)
        // let mut length = 0.0;
        // for e in self.0.iter().flattened(tolerance) {
        //     match e {
        //         PathEvent::Line { from, to } => {
        //             length += (to - from).length();
        //         }
        //         _ => {}
        //     }
        // }
        // length
    }
}

pub trait GetPartial: MeasureLength {
    fn upto(self, ratio: f32, tolerance: f32) -> Path;
}

impl GetPartial for Path {
    fn upto(self, ratio: f32, tolerance: f32) -> Path {
        if ratio >= 1.0 {
            self
        } else {
            let ratio = ratio.max(0.0);
            let full_length = self.approximate_length(tolerance);
            let stop_at = ratio * full_length;

            let mut builder = Path::svg_builder();
            let mut length = 0.0;

            for e in self.0.iter().flattened(tolerance) {
                if length > stop_at {
                    break;
                }
                match e {
                    PathEvent::Begin { at } => {
                        builder.move_to(at);
                    }
                    PathEvent::Line { from, to } => {
                        let seg_length = (to - from).length();
                        let new_length = length + seg_length;
                        if new_length > stop_at {
                            let seg_ratio = 1.0 - (new_length - stop_at) / seg_length;
                            builder.line_to(from.lerp(to, seg_ratio));
                            break;
                        } else {
                            length = new_length;
                            builder.line_to(to);
                        }
                    }
                    PathEvent::End { .. } => {
                        builder.close();
                    }
                    _ => (),
                }
            }
            Self(builder.build())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // use nannou::geom::rect::Rect;
    // use nannou::lyon::path::builder::PathBuilder;
    use nannou::lyon::math::{point, Point};
    use nannou::prelude::*;

    #[test]
    fn partial_path() {
        let win_rect = Rect::from_w_h(640.0, 480.0);
        let text = text("Hello").font_size(128).left_justify().build(win_rect);
        let mut builder = Path::builder();
        for e in text.path_events() {
            builder.path_event(e);
        }
        builder.close();
        let path = Path(builder.build());
        let partial_path = path.upto(0.5, 0.01);

        println!("length = {}", partial_path.approximate_length(0.01));
    }

    #[test]
    fn flatten() {
        let mut builder = Path::svg_builder();
        builder.move_to(point(0.0, 0.0));
        builder.line_to(point(10.0, 0.0));
        builder.close();
        let mut path = Path(builder.build()).upto(0.5, 0.01);
        for e in path.0.iter().flattened(0.01) {
            match e {
                PathEvent::Begin { at } => {}
                PathEvent::Line { from, to } => {
                    println!("from:({},{}), to:({},{})", from.x, from.y, to.x, to.y);
                }
                PathEvent::End { .. } => {}
                _ => (),
            }
        }
    }

    #[test]
    fn iter_check() {
        let arr = [1, 2, 3, 4, 5];
        let out = arr
            .iter()
            .zip(arr.iter().skip(1))
            .scan(0, |val, a| {
                *val += a.0 + a.1;
                Some(*val)
            })
            .collect::<Vec<i32>>();

        dbg!(out);
    }

    #[test]
    fn length() {
        use nannou::lyon::algorithms::length::approximate_length;

        let mut builder = Path::svg_builder();
        builder.move_to(point(0.0, 0.0));
        builder.line_to(point(10.0, 0.0));
        builder.quadratic_bezier_to(point(15.0, 5.0), point(20.0, 0.0));
        builder.close();

        let path = Path(builder.build());
        let l = approximate_length(path.0.iter(), 0.01);
        let l2 = path.approximate_length(0.01);

        println!("{}, {}", l, l2);
    }

    #[test]
    fn check_vector_ordering() {
        let v1 = vec![0.0, 0.3, 0.6, 0.8, 1.0];
        let v2 = vec![0.2, 0.5, 0.55, 0.8, 2.0];

        let out = combine_vectors_with_ordering(&v1, &v2);
        assert_eq!(*out, vec![0.0, 0.1, 0.25, 0.275, 0.3, 0.4, 0.6, 0.8, 1.0]);
    }

    #[test]
    fn check_walk() {
        let mut builder = Path::builder();
        builder.begin(point(5.0, 5.0));
        builder.line_to(point(5.0, 10.0));
        builder.line_to(point(10.0, 10.0));
        builder.line_to(point(10.0, 5.0));
        builder.end(true);
        let path = builder.build();

        let pts = vec![0.0, 2.0, 2.5, 5.0, 10.0, 20.0];
        let pts: Vec<f32> = pts
            .iter()
            .zip(pts.iter().skip(1))
            .map(|(a, b)| b - a)
            .collect();

        let mut pattern = RepeatedPattern {
            callback: &mut |position: Point, _t, d| {
                println!("d = {}, x = {}, y = {}", d, position.x, position.y);
                true
            },
            intervals: &pts,
            index: 0,
        };

        walk_along_path(path.iter(), 0.0, &mut pattern);
    }
    #[test]
    fn circle() {
        use nannou::lyon::math::{Angle, Vector};
        let mut builder = Path::svg_builder();

        let radius = 3.0;
        let sweep_angle = Angle::radians(-TAU);
        let x_rotation = Angle::radians(0.0);
        let center = point(0.0, 0.0);
        let start = point(radius, 0.0);
        let radii = Vector::new(radius, radius);

        builder.move_to(start);
        builder.arc(center, radii, sweep_angle, x_rotation);
        builder.close();

        // let mut path = Path(builder.build()).upto(0.5, 0.01);
        for e in builder.build().iter() {
            match e {
                PathEvent::Begin { at } => {
                    println!("Begin -> at:({},{})", at.x, at.y);
                }
                PathEvent::Line { from, to } => {
                    println!(
                        "Line -> from:({},{}), to:({},{})",
                        from.x, from.y, to.x, to.y
                    );
                }
                PathEvent::Quadratic { from, ctrl, to } => {
                    println!(
                        "Quadratic -> from:({},{}), to:({},{})",
                        from.x, from.y, to.x, to.y
                    );
                }
                PathEvent::Cubic {
                    from,
                    ctrl1,
                    ctrl2,
                    to,
                } => {
                    println!(
                        "Cubic -> from:({},{}), to:({},{})",
                        from.x, from.y, to.x, to.y
                    );
                }
                PathEvent::End { .. } => {
                    println!("End");
                }
                _ => (),
            }
        }
    }
}
