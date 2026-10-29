//! What the user shares of the work open in the app: the active tab's editor text, selection
//! and result. The user decides in the app, each time or once for the agent's session; this
//! side keeps the limits whatever the app sends, and never returns what belongs to a
//! connection agents may not use.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::{Agent, origin_name};
use crate::error::{AgentError, Result};
use crate::link::{
    CLOSED, ContextPart, ContextRequest, ContextResult, ContextVerdict, UserContext,
};

/// Bytes of the editor text, and of the selection, `get_user_context` returns at most.
pub const CONTEXT_TEXT_BYTES: usize = 64 * 1024;

/// Rows, and bytes of text in them, of the result it returns at most.
pub const CONTEXT_ROWS: usize = 100;
pub const CONTEXT_RESULT_BYTES: usize = 32 * 1024;

/// Bytes of one cell of the result: a long value must not take the room of the rows.
const CONTEXT_CELL_BYTES: usize = 1024;

/// The arguments of `get_user_context`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ContextArgs {
    /// What to ask for: any of `editor`, `selection` and `result`; all three when left out.
    pub parts: Option<Vec<ContextPart>>,
}

/// What `get_user_context` returns: what the user shared, and what was cut from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SharedContext {
    #[serde(flatten)]
    pub context: UserContext,
    /// What the limits cut, in words.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// `text` cut to at most `max` bytes at a character boundary; `true` when it was cut.
fn cut(mut text: String, max: usize) -> (String, bool) {
    if text.len() <= max {
        return (text, false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    (text, true)
}

/// `context` limited to the `parts` asked for and to the sizes above.
fn limited(mut context: UserContext, parts: &[ContextPart]) -> SharedContext {
    let mut notes = Vec::new();
    if !parts.contains(&ContextPart::Editor) {
        context.editor = None;
    }
    if !parts.contains(&ContextPart::Selection) {
        context.selection = None;
    }
    if !parts.contains(&ContextPart::Result) {
        context.result = None;
    }
    for (name, text) in [
        ("editor", &mut context.editor),
        ("selection", &mut context.selection),
    ] {
        if let Some(whole) = text.take() {
            let (kept, was_cut) = cut(whole, CONTEXT_TEXT_BYTES);
            if was_cut {
                notes.push(format!(
                    "the {name} was cut at {CONTEXT_TEXT_BYTES} bytes: the text goes on"
                ));
            }
            *text = Some(kept);
        }
    }
    if let Some(result) = &mut context.result {
        limit_result(result, &mut notes);
    }
    SharedContext { context, notes }
}

/// `result` cut to [`CONTEXT_ROWS`] rows and [`CONTEXT_RESULT_BYTES`] bytes, with a note for
/// what the cut took.
fn limit_result(result: &mut ContextResult, notes: &mut Vec<String>) {
    let sent = result.rows.len();
    // The app's count of the whole result, when it has one, else what it sent.
    let known_total = result.total_rows.map(|t| t.max(sent));
    let total = known_total.unwrap_or(sent);
    let mut left =
        CONTEXT_RESULT_BYTES.saturating_sub(result.columns.iter().map(String::len).sum());
    let (mut kept, mut cells_cut) = (Vec::new(), false);
    for row in result.rows.drain(..).take(CONTEXT_ROWS) {
        let mut size = 0;
        let row: Vec<Option<String>> = row
            .into_iter()
            .map(|cell| {
                cell.map(|text| {
                    let (text, was_cut) = cut(text, CONTEXT_CELL_BYTES);
                    cells_cut |= was_cut;
                    size += text.len();
                    text
                })
            })
            .collect();
        if size > left {
            break;
        }
        left -= size;
        kept.push(row);
    }
    if kept.len() < total {
        result.truncated = true;
        let of = if known_total.is_some() {
            format!("of its {total} rows")
        } else {
            "of the rows shared".to_owned()
        };
        notes.push(format!(
            "the result was cut to {} {of} (at most {CONTEXT_ROWS} rows and {CONTEXT_RESULT_BYTES} bytes)",
            kept.len()
        ));
    }
    if cells_cut {
        notes.push(format!(
            "values longer than {CONTEXT_CELL_BYTES} bytes were cut"
        ));
    }
    result.rows = kept;
}

impl Agent {
    /// What the user shares of the work open in the app, after they allow it there.
    /// Nothing is returned from a tab on a connection agents may not use.
    pub async fn user_context(
        &self,
        args: &ContextArgs,
        (origin, session): (&str, &str),
    ) -> Result<SharedContext> {
        let Some(link) = self.link().filter(|link| link.reachable()) else {
            return Err(AgentError::Store(CLOSED.to_owned()));
        };
        let parts = args
            .parts
            .clone()
            .filter(|parts| !parts.is_empty())
            .unwrap_or_else(|| ContextPart::ALL.to_vec());
        let request = ContextRequest {
            id: Uuid::new_v4().to_string(),
            agent: origin_name(origin),
            session: session.to_owned(),
            parts: parts.clone(),
        };
        let context = match link.context(request).await {
            Ok(ContextVerdict::Once { context } | ContextVerdict::Session { context }) => context,
            Ok(ContextVerdict::Deny { reason }) => {
                return Err(AgentError::Denied(match reason {
                    Some(reason) if !reason.trim().is_empty() => format!(
                        "the user did not share what they are working on: {}",
                        reason.trim()
                    ),
                    _ => "the user did not share what they are working on".to_owned(),
                }));
            }
            Err(e) if e == CLOSED => return Err(AgentError::Store(CLOSED.to_owned())),
            Err(e) => {
                return Err(AgentError::Store(format!(
                    "cannot ask the user in PlatypusDB: {e}"
                )));
            }
        };
        self.check_open_to_agents(&context)?;
        Ok(limited(context, &parts))
    }

    /// Refuses a context whose tab is on a connection agents may not use (access off, or not
    /// saved). The id is the safe key; the name is all there is when the app sent no id.
    fn check_open_to_agents(&self, context: &UserContext) -> Result<()> {
        let key = context
            .connection_id
            .map(|id| id.to_string())
            .or_else(|| context.connection.clone());
        let Some(key) = key else {
            return Ok(());
        };
        match self.find(&key) {
            Ok(_) => Ok(()),
            Err(AgentError::Store(e)) => Err(AgentError::Store(e)),
            Err(_) => Err(AgentError::Denied(
                "the user's active tab is on a connection that is not open to agents, so its \
                 content is not shared"
                    .to_owned(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(editor: &str, rows: usize) -> UserContext {
        UserContext {
            editor: Some(editor.to_owned()),
            selection: Some("sel".to_owned()),
            result: Some(ContextResult {
                columns: vec!["n".to_owned()],
                rows: (0..rows).map(|i| vec![Some(i.to_string())]).collect(),
                truncated: false,
                total_rows: None,
            }),
            ..UserContext::default()
        }
    }

    #[test]
    fn text_is_cut_at_a_character_boundary_and_says_so() {
        // "é" is two bytes: a cut in the middle of one backs off.
        let text = "é".repeat(CONTEXT_TEXT_BYTES / 2 + 1);
        let shared = limited(context(&text, 0), &ContextPart::ALL);
        let editor = shared.context.editor.unwrap();
        assert_eq!(editor.len(), CONTEXT_TEXT_BYTES);
        assert!(shared.notes[0].contains("editor"), "{:?}", shared.notes);
        let odd = format!("a{}", "é".repeat(CONTEXT_TEXT_BYTES));
        let shared = limited(context(&odd, 0), &ContextPart::ALL);
        assert_eq!(shared.context.editor.unwrap().len(), CONTEXT_TEXT_BYTES - 1);
    }

    #[test]
    fn rows_are_limited_in_number_and_bytes() {
        let shared = limited(context("x", 250), &ContextPart::ALL);
        let result = shared.context.result.unwrap();
        assert_eq!(result.rows.len(), CONTEXT_ROWS);
        assert!(result.truncated);
        // The app did not say how many rows the result has: only what was shared is known.
        assert!(
            shared
                .notes
                .iter()
                .any(|n| n.contains("cut to 100 of the rows shared")),
            "{:?}",
            shared.notes
        );

        // The app sent its first 100 of 150: the note carries the real size.
        let mut sent = context("x", 100);
        let known = sent.result.as_mut().unwrap();
        known.total_rows = Some(150);
        known.truncated = true;
        let shared = limited(sent, &ContextPart::ALL);
        assert!(
            shared
                .notes
                .iter()
                .any(|n| n.contains("cut to 100 of its 150 rows")),
            "{:?}",
            shared.notes
        );
        // All rows sent and counted: nothing was cut.
        let mut whole = context("x", 5);
        whole.result.as_mut().unwrap().total_rows = Some(5);
        assert_eq!(
            limited(whole, &ContextPart::ALL).notes,
            Vec::<String>::new()
        );

        let mut wide = context("x", 0);
        let big = || vec![Some("y".repeat(CONTEXT_CELL_BYTES))];
        wide.result.as_mut().unwrap().rows = (0..60).map(|_| big()).collect();
        let result = limited(wide, &ContextPart::ALL).context.result.unwrap();
        assert!(result.rows.len() < 60 && result.truncated);
        let bytes: usize = result
            .rows
            .iter()
            .flatten()
            .flatten()
            .map(String::len)
            .sum();
        assert!(bytes <= CONTEXT_RESULT_BYTES);

        let mut long = context("x", 1);
        long.result.as_mut().unwrap().rows[0][0] = Some("z".repeat(5000));
        let shared = limited(long, &ContextPart::ALL);
        assert_eq!(
            shared.context.result.unwrap().rows[0][0]
                .as_ref()
                .unwrap()
                .len(),
            CONTEXT_CELL_BYTES
        );
        assert!(shared.notes.iter().any(|n| n.contains("values longer")));
    }

    #[test]
    fn only_the_parts_asked_for_are_returned() {
        let shared = limited(context("x", 1), &[ContextPart::Selection]);
        assert_eq!(shared.context.editor, None);
        assert_eq!(shared.context.selection.as_deref(), Some("sel"));
        assert_eq!(shared.context.result, None);
        assert_eq!(shared.notes, Vec::<String>::new());
    }
}
