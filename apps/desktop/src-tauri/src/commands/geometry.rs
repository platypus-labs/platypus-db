//! Geometries of loaded cells for the map panel: binary values read as (E)WKB, text as WKT,
//! hex EWKB or a PostgreSQL geometric type.

use std::fmt::Write as _;

use arrow_array::{Array, RecordBatch};
use arrow_cast::display::ArrayFormatter;
use platypus_core::geometry::{self, Shape};
use serde::Serialize;
use tauri::State;

use super::AppState;
use super::grid::binary_at;
use super::summary::{blocking, check_columns};
use crate::display;
use crate::error::{CommandError, CommandResult};

/// Shapes reported at most by [`geometries`].
const SHAPE_LIMIT: usize = 10_000;

#[derive(Debug, Serialize, PartialEq)]
pub struct Located {
    /// Row of the result, from 0.
    row: usize,
    column: usize,
    shape: Shape,
}

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Geometries {
    shapes: Vec<Located>,
    /// Cells that are not NULL and hold no geometry.
    unreadable: usize,
    /// More cells had a geometry than were reported.
    truncated: bool,
}

fn geometries_of(
    batches: &[RecordBatch],
    offset: usize,
    columns: &[usize],
) -> Result<Geometries, arrow_schema::ArrowError> {
    let options = display::format_options();
    let mut out = Geometries::default();
    let mut first = offset;
    let mut text = String::new();
    for batch in batches {
        for &column in columns {
            let array = batch.column(column);
            let formatter = ArrayFormatter::try_new(array.as_ref(), &options)?;
            for row in (0..array.len()).filter(|&r| !array.is_null(r)) {
                let shape = if let Some(bytes) = binary_at(array.as_ref(), row) {
                    geometry::from_bytes(bytes)
                } else {
                    text.clear();
                    let _ = write!(text, "{}", formatter.value(row));
                    geometry::from_text(&text)
                };
                match shape {
                    None => out.unreadable += 1,
                    Some(_) if out.shapes.len() == SHAPE_LIMIT => out.truncated = true,
                    Some(shape) => out.shapes.push(Located {
                        row: first + row,
                        column,
                        shape,
                    }),
                }
            }
        }
        first += batch.num_rows();
    }
    out.shapes.sort_by_key(|s| (s.row, s.column));
    Ok(out)
}

/// The geometries in `columns` of loaded rows `offset..offset + limit`.
#[tauri::command]
pub async fn geometries(
    state: State<'_, AppState>,
    job_id: u64,
    offset: usize,
    limit: usize,
    columns: Vec<usize>,
) -> CommandResult<Geometries> {
    let job = state.job(job_id)?;
    blocking(move || {
        check_columns(&job, &columns)?;
        let (_, batches) = job
            .window(offset, limit)
            .ok_or_else(|| CommandError::new("not_ready", "query has no result set yet"))?;
        Ok(geometries_of(&batches, offset, &columns)?)
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::{BinaryArray, StringArray};
    use arrow_schema::{DataType, Field, Schema};

    use super::*;

    fn wkb_point(x: f64, y: f64) -> Vec<u8> {
        let mut b = vec![1];
        b.extend(1_u32.to_le_bytes());
        b.extend(x.to_le_bytes());
        b.extend(y.to_le_bytes());
        b
    }

    #[test]
    fn reads_binary_and_text_cells() {
        let schema = Arc::new(Schema::new(vec![
            Field::new("wkb", DataType::Binary, true),
            Field::new("wkt", DataType::Utf8, true),
        ]));
        let point = wkb_point(1.0, 2.0);
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(BinaryArray::from(vec![Some(point.as_slice()), None])),
                Arc::new(StringArray::from(vec![Some("hello"), Some("POINT (3 4)")])),
            ],
        )
        .unwrap();
        let g = geometries_of(&[batch], 10, &[1, 0]).unwrap();
        assert_eq!(g.unreadable, 1);
        assert!(!g.truncated);
        assert_eq!(
            g.shapes,
            vec![
                Located {
                    row: 10,
                    column: 0,
                    shape: Shape::Point { x: 1.0, y: 2.0 }
                },
                Located {
                    row: 11,
                    column: 1,
                    shape: Shape::Point { x: 3.0, y: 4.0 }
                },
            ]
        );
    }
}
