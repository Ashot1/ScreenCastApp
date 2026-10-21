#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AmbientText {
    pub bytes: *const u8,
    pub length: u32,
}
impl Default for AmbientText {
    fn default() -> Self {
        Self {
            bytes: std::ptr::null(),
            length: 0,
        }
    }
}
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct AmbientSpan {
    pub offset: u32,
    pub length: u32,
}
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct AmbientTask {
    pub id: u64,
    pub revision: u64,
    pub text: AmbientSpan,
    pub lifecycle: u32,
    pub initiative_suppressed: u32,
    pub plan_completeness: u32,
}
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct AmbientTaskVersion {
    pub task_id: u64,
    pub revision: u64,
}
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct AmbientPage {
    pub next_after: u64,
    pub count: u32,
    pub has_more: u32,
    pub bytes_used: u32,
    pub task_revision: u64,
    pub plan_completeness: u32,
}
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct AmbientEvidence {
    pub id: u64,
    pub task_id: u64,
    pub observed_at_utc: i64,
    pub has_observed_at: u32,
    pub origin: u32,
    pub completeness: u32,
    pub kind: u32,
    pub text: AmbientSpan,
    pub namespace: AmbientSpan,
    pub scope: AmbientSpan,
    pub object_id: AmbientSpan,
    pub has_version: u32,
    pub version: AmbientSpan,
}
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct AmbientSource {
    pub id: u64,
    pub task_id: u64,
    pub evidence_id: u64,
    pub namespace: AmbientSpan,
    pub scope: AmbientSpan,
    pub object_id: AmbientSpan,
    pub has_version: u32,
    pub version: AmbientSpan,
}
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct AmbientLink {
    pub id: u64,
    pub other_task_id: u64,
    pub action_at_utc: i64,
    pub related: u32,
    pub other_task_revision: u64,
    pub other_task_text: AmbientSpan,
    pub other_task_lifecycle: u32,
    pub other_task_initiative_suppressed: u32,
    pub other_task_plan_completeness: u32,
}
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct AmbientDecision {
    pub id: u64,
    pub evidence_id: u64,
    pub action_at_utc: i64,
    pub accepted: u32,
}
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct AmbientSourceKey {
    pub namespace: AmbientText,
    pub scope: AmbientText,
    pub object_id: AmbientText,
    pub has_version: u32,
    pub version: AmbientText,
}
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct AmbientSourceEvidence {
    pub text: AmbientText,
    pub observed_at_utc: i64,
    pub has_observed_at: u32,
    pub completeness: u32,
    pub kind: u32,
}
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct AmbientIngest {
    pub evidence_id: u64,
    pub outcome: u32,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct AmbientStepInput {
    pub description: AmbientText,
    pub has_prompt: u32,
    pub prompt: AmbientText,
    pub has_reference: u32,
    pub reference: AmbientText,
}
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct AmbientStep {
    pub id: u64,
    pub task_id: u64,
    pub description: AmbientSpan,
    pub has_prompt: u32,
    pub prompt: AmbientSpan,
    pub has_reference: u32,
    pub reference: AmbientSpan,
    pub has_completed: u32,
    pub completed_at_utc: i64,
    pub ready: u32,
}
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct AmbientCheck {
    pub id: u64,
    pub task_id: u64,
    pub step_id: u64,
    pub description: AmbientSpan,
    pub result: u32,
    pub has_confirmation: u32,
    pub confirmed_at_utc: i64,
}
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct AmbientDependency {
    pub id: u64,
    pub task_id: u64,
    pub predecessor: u64,
    pub dependent: u64,
}
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct AmbientReference {
    pub id: u64,
    pub task_id: u64,
    pub text: AmbientSpan,
}
