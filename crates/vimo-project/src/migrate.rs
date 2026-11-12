//! Forward migrations.
//!
//! A project file written by an older build is migrated forward, one version
//! at a time, until it matches the version this build understands. Each step
//! is a small, self-contained function so that a bug in one step cannot
//! corrupt the others.
//!
//! The migrations are *semantic*: the binary layout differences between
//! versions are handled by the version-aware reader in [`crate::codec`], while
//! this module deals with meaning (for example "a negative source in-point
//! used to mean the clip plays backwards").

use vimo_foundation::error::{ErrorKind, VimoError, VimoResult};
use vimo_manifest::EffectRegistry;

use crate::codec::CURRENT_VERSION;
use crate::project::Project;

/// What a migration changed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MigrationReport {
    /// The version the project started at.
    pub from_version: u32,
    /// The version the project ended at.
    pub to_version: u32,
    /// Number of parameter values coerced to their declared type.
    pub coerced_params: usize,
    /// Number of clips whose playback direction was normalised.
    pub flipped_clips: usize,
    /// Number of entities that were dropped.
    pub dropped: usize,
}

/// Migrate `project` from `from` to [`CURRENT_VERSION`].
pub fn migrate(
    project: &mut Project,
    from: u32,
    registry: &EffectRegistry,
    mut report: crate::codec::LoadReport,
) -> VimoResult<crate::codec::LoadReport> {
    let mut version = from;
    while version < CURRENT_VERSION {
        match version {
            1 => {
                let flipped = migrate_1_to_2(project);
                report.migrated_to = Some(2);
                version = 2;
                let _ = flipped;
            }
            2 => {
                let coerced = migrate_2_to_3(project, registry);
                report.coerced_params += coerced;
                report.migrated_to = Some(3);
                version = 3;
            }
            other => {
                return Err(VimoError::detailed(
                    ErrorKind::Unsupported,
                    "migrate",
                    format!("no migration path from version {other}"),
                ))
            }
        }
    }
    if report.migrated_to.is_some() {
        report.migrated_to = Some(CURRENT_VERSION);
    }
    Ok(report)
}

/// v1 → v2.
///
/// Version 1 had no `forward` flag; a reversed clip was encoded with a
/// negative source in-point.
pub fn migrate_1_to_2(project: &mut Project) -> usize {
    let mut flipped = 0;
    for clip in &mut project.clips {
        if clip.source_in.as_nanos() < 0 {
            clip.source_in = neg_time(clip.source_in);
            clip.forward = false;
            flipped += 1;
        }
    }
    flipped
}

/// v2 → v3.
///
/// Version 3 introduced typed effect parameters. Values written by v2 are
/// floats regardless of the declared type, so they are coerced to the type the
/// manifest declares (and clamped into range).
pub fn migrate_2_to_3(project: &mut Project, registry: &EffectRegistry) -> usize {
    let mut coerced = 0;
    for effect in &mut project.effects {
        let Some(desc) = registry.get(effect.effect_type) else {
            continue;
        };
        let overrides: Vec<(u16, vimo_manifest::ParamValue)> =
            effect.params.iter_overridden().collect();
        for (index, value) in overrides {
            let Some(p) = desc.param(index) else {
                continue;
            };
            let c = p.coerce(value);
            if c != value {
                coerced += 1;
            }
            effect.params.set(index, c);
        }
    }
    coerced
}

/// Helper: negate a time value.
fn neg_time(t: vimo_foundation::time::Time) -> vimo_foundation::time::Time {
    vimo_foundation::time::Time::from_nanos(-t.as_nanos())
}

#[cfg(test)]
mod tests {
    use super::*;
    use vimo_foundation::time::Time;

    #[test]
    fn v1_negative_source_in_becomes_a_reverse_clip() {
        let mut p = Project::new();
        let mut clip = crate::model::Clip::new(
            0,
            vimo_foundation::time::TimeRange::new(Time::ZERO, Time::from_secs(1)),
            Time::from_nanos(-500),
        );
        clip.forward = true;
        p.clips.push(clip);
        let flipped = migrate_1_to_2(&mut p);
        assert_eq!(flipped, 1);
        assert!(!p.clips[0].forward);
        assert_eq!(p.clips[0].source_in, Time::from_nanos(500));
    }

    #[test]
    fn v2_float_params_become_typed() {
        let mut reg = EffectRegistry::new();
        let desc = vimo_manifest::EffectDesc::new(
            "x",
            "x",
            "x",
            1,
            None,
            vec![vimo_manifest::ParamDesc {
                name: "count".into(),
                ptype: vimo_manifest::ParamType::Int,
                default: vimo_manifest::ParamValue::Int(1),
                min: None,
                max: None,
                step: None,
                label: "Count".into(),
                animatable: true,
                options: Vec::new(),
            }],
            Default::default(),
        )
        .unwrap();
        reg.register(desc).unwrap();

        let mut p = Project::new();
        let mut node = crate::model::EffectNode::new(
            vimo_manifest::EffectTypeId::from_index(0),
            reg.get(vimo_manifest::EffectTypeId::from_index(0)).unwrap(),
        );
        node.params.set(0, vimo_manifest::ParamValue::Float(4.0));
        p.effects.push(node);

        let coerced = migrate_2_to_3(&mut p, &reg);
        assert_eq!(coerced, 1);
        assert_eq!(
            p.effects[0].params.get(0),
            Some(vimo_manifest::ParamValue::Int(4))
        );
    }
}
