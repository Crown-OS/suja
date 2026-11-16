#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolOutcome {
    pub ok: bool,
    pub spoken_summary: String,
    pub detail_for_model: String,
    pub needs_confirmation: bool,
}

impl ToolOutcome {
    pub fn success(spoken_summary: impl Into<String>, detail_for_model: impl Into<String>) -> Self {
        Self {
            ok: true,
            spoken_summary: spoken_summary.into(),
            detail_for_model: detail_for_model.into(),
            needs_confirmation: false,
        }
    }

    pub fn failure(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            ok: false,
            spoken_summary: reason.clone(),
            detail_for_model: format!("error: {reason}"),
            needs_confirmation: false,
        }
    }

    pub fn needs_confirmation(question: impl Into<String>) -> Self {
        let question = question.into();
        Self {
            ok: false,
            spoken_summary: question.clone(),
            detail_for_model: format!(
                "confirmation required before running: {question} Ask the user to confirm with yes; do not claim it ran."
            ),
            needs_confirmation: true,
        }
    }
}
