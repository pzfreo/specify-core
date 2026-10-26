//! Measured answer-ID changes in quiddity v0.2.1-alpha.1 (#100).
use haecceity::geom::Surface;
use serde_json::{Value, json};
use specify_core::part::LoadedPart;

pub const FTC10: &[(&str, &str)] = &[
    ("holes:175", "holes:174.175"),
    ("holes:177", "holes:176.177"),
    (
        "hole_patterns.linear:138.140.142.144.146.147.148.149",
        "hole_patterns.linear:130.132.134.136.138.140.142.144.146.147.148.149",
    ),
    (
        "hole_patterns.linear:189.191.193",
        "hole_patterns.linear:188.189.190.191.192.193",
    ),
];

pub const CTC02: &[(&str, &str)] = &[
    (
        "hole_patterns.linear:13.14.17.18.21.22.25.26.29.30",
        "hole_patterns.linear:13.14.15.16.17.18.19.20.21.22.23.24.25.26.27.28.29.30.31.32",
    ),
    (
        "hole_patterns.linear:61.62.65.66.69.70",
        "hole_patterns.linear:61.62.63.64.65.66.67.68.69.70.71.72",
    ),
    (
        "hole_patterns.linear:470.471.474.475.478.479.482.483.546.547",
        "hole_patterns.linear:470.471.472.473.474.475.476.477.478.479.480.481.482.483.484.485.546.547.548.549",
    ),
    (
        "hole_patterns.linear:490.491.494.495.530.531.534.535",
        "hole_patterns.linear:490.491.492.493.494.495.496.497.530.531.532.533.534.535.536.537",
    ),
    (
        "hole_patterns.bolt_circle:244.245.256.257.276.277",
        "hole_patterns.bolt_circle:244.245.246.247.256.257.258.259.276.277.278.279",
    ),
    (
        "hole_patterns.set:33.34.37.38.53.54.57.58.93.94.97.98.113.114.117.118",
        "hole_patterns.set:33.34.35.36.37.38.39.40.53.54.55.56.57.58.59.60.93.94.95.96.97.98.99.100.113.114.115.116.117.118.119.120",
    ),
    (
        "hole_patterns.set:655.656.659.660",
        "hole_patterns.set:655.656.657.658.659.660.661.662",
    ),
    (
        "hole_patterns.set:252.253.264.265.268.269.280.281",
        "hole_patterns.set:252.253.254.255.264.265.266.267.268.269.270.271.280.281.282.283",
    ),
];

/// Exact answer moves, plus independent material support: the additional tip cones must
/// touch the old bore faces. This cannot silently accept another hole or pattern member.
/// FTC-10's four counterbore cylinders were already added by schema 6 (#81/#82); the
/// remaining moves are #100 drill-point cone claims, not a change of physical feature.
pub fn check(loaded: &LoadedPart, base: &Value, analysis: &Value, mapping: &[(&str, &str)]) {
    let expected: Vec<_> = base["answers"]
        .as_object()
        .unwrap()
        .keys()
        .filter_map(|id| {
            let (question, old) = id.split_once(':')?;
            let (_, new) = mapping.iter().find(|(from, _)| *from == old)?;
            Some(json!([id, format!("{question}:{new}")]))
        })
        .collect();
    assert_eq!(analysis["resume"]["migrated"], json!(expected));
    for (old, new) in mapping {
        let old = base["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == *old)
            .unwrap();
        let new = analysis["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == *new)
            .unwrap();
        assert_eq!(old["family"], new["family"]);
        let faces = |f: &Value| -> Vec<usize> {
            f["faces"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as usize)
                .collect()
        };
        let before = faces(old);
        let after = faces(new);
        assert!(before.iter().all(|f| after.contains(f)));
        let extra: Vec<_> = after
            .iter()
            .filter(|f| !before.contains(f))
            .copied()
            .collect();
        assert!(!extra.is_empty());
        if old["id"] == "hole_patterns.linear:138.140.142.144.146.147.148.149" {
            assert_eq!(extra, [130, 132, 134, 136]);
            assert!(
                extra
                    .iter()
                    .all(|&f| matches!(loaded.part.faces[f].surface, Surface::Cylinder { .. }))
            );
        } else {
            assert!(
                extra.iter().all(
                    |&f| matches!(loaded.part.faces[f].surface, Surface::Cone { .. })
                        && loaded.part.neighbours(f).iter().any(|g| before.contains(g))
                ),
                "{} -> {}",
                old["id"],
                new["id"]
            );
        }
    }
    for pair in expected {
        let old = pair[0].as_str().unwrap();
        let new = pair[1].as_str().unwrap();
        assert_eq!(analysis["resume"]["answers"][new], base["answers"][old]);
    }
}
