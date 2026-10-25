//! ADBC 0.24 hands out arrow 59 batches; the app works with arrow 60. A batch crosses over
//! as an Arrow IPC stream: written with one version, read back with the other.

use std::io::Cursor;
use std::sync::Arc;

use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use platypus_core::{Error, Result};

fn decode_error(e: impl std::fmt::Display) -> Error {
    Error::Unsupported(format!("cannot read the driver's data: {e}"))
}

/// Writes `batches` of `schema` as an IPC stream with arrow 59.
fn encode(
    schema: &arrow_schema_59::Schema,
    batches: &[arrow_array_59::RecordBatch],
) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut writer =
        arrow_ipc_59::writer::StreamWriter::try_new(&mut buf, schema).map_err(decode_error)?;
    for batch in batches {
        writer.write(batch).map_err(decode_error)?;
    }
    writer.finish().map_err(decode_error)?;
    drop(writer);
    Ok(buf)
}

/// Reads an IPC stream with arrow 60.
fn decode(bytes: Vec<u8>) -> Result<(SchemaRef, Vec<RecordBatch>)> {
    let reader =
        arrow_ipc::reader::StreamReader::try_new(Cursor::new(bytes), None).map_err(decode_error)?;
    let schema = reader.schema();
    let batches = reader
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(decode_error)?;
    Ok((schema, batches))
}

/// `schema` in arrow 60.
pub fn schema(schema: &arrow_schema_59::Schema) -> Result<SchemaRef> {
    Ok(decode(encode(schema, &[])?)?.0)
}

/// `batch` in arrow 60.
pub fn batch(batch: &arrow_array_59::RecordBatch) -> Result<RecordBatch> {
    let (schema, batches) = decode(encode(&batch.schema(), std::slice::from_ref(batch))?)?;
    Ok(batches
        .into_iter()
        .next()
        .unwrap_or_else(|| RecordBatch::new_empty(Arc::clone(&schema))))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow_array::Array as _;
    use arrow_array_59::{ArrayRef, Int32Array, StringArray};

    #[test]
    fn ipc_round_trip() {
        let schema = Arc::new(arrow_schema_59::Schema::new(vec![
            arrow_schema_59::Field::new("id", arrow_schema_59::DataType::Int32, false),
            arrow_schema_59::Field::new("name", arrow_schema_59::DataType::Utf8, true),
        ]));
        let old = arrow_array_59::RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(Int32Array::from(vec![1, 2])) as ArrayRef,
                Arc::new(StringArray::from(vec![Some("a"), None])),
            ],
        )
        .unwrap();
        let new = super::batch(&old).unwrap();
        assert_eq!(new.num_rows(), 2);
        assert_eq!(new.schema().field(1).name(), "name");
        let names = new
            .column(1)
            .as_any()
            .downcast_ref::<arrow_array::StringArray>()
            .unwrap();
        assert_eq!(names.value(0), "a");
        assert!(names.is_null(1));
        assert_eq!(super::schema(&schema).unwrap().fields().len(), 2);
    }
}
