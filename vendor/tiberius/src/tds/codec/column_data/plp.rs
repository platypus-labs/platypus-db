use futures_util::io::AsyncReadExt;

use crate::sql_read_bytes::SqlReadBytes;

// Decode a partially length-prefixed type.
//
// PlatypusDB: reads each chunk whole instead of a byte at a time, an `await` per byte: a
// 32 MB `varchar(max)` took 10 s instead of 0.3 s.
pub(crate) async fn decode<R>(src: &mut R, len: usize) -> crate::Result<Option<Vec<u8>>>
where
    R: SqlReadBytes + Unpin,
{
    match len {
        // Fixed size
        len if len < 0xffff => {
            let len = src.read_u16_le().await? as usize;

            match len {
                // NULL
                0xffff => Ok(None),
                _ => {
                    let mut data = vec![0; len];
                    src.read_exact(&mut data).await?;

                    Ok(Some(data))
                }
            }
        }
        // Unknown size, length-prefixed blobs
        _ => {
            let len = src.read_u64_le().await?;

            let mut data = match len {
                // NULL
                0xffffffffffffffff => return Ok(None),
                // Unknown size
                0xfffffffffffffffe => Vec::new(),
                // Known size
                _ => Vec::with_capacity(len as usize),
            };

            loop {
                let chunk_size = src.read_u32_le().await? as usize;

                if chunk_size == 0 {
                    break; // found a sentinel, we're done
                }

                let start = data.len();
                data.resize(start + chunk_size, 0);
                src.read_exact(&mut data[start..]).await?;
            }

            Ok(Some(data))
        }
    }
}
