use crate::{error::Error, numeric::Numeric, sql_read_bytes::SqlReadBytes, ColumnData};

pub(crate) async fn decode<R>(src: &mut R, len: u8) -> crate::Result<ColumnData<'static>>
where
    R: SqlReadBytes + Unpin,
{
    // PlatypusDB: money is a count of ten-thousandths, read exactly as a numeric with
    // four decimals; as a float, values above 2^53 / 10^4 lost their last digits.
    let money = |units: i64| ColumnData::Numeric(Some(Numeric::new_with_scale(units.into(), 4)));
    let res = match len {
        0 => ColumnData::Numeric(None),
        4 => money(src.read_i32_le().await? as i64),
        8 => {
            let high = src.read_i32_le().await? as i64;
            let low = src.read_u32_le().await? as i64;
            money((high << 32) | low)
        }
        _ => {
            return Err(Error::Protocol(
                format!("money: length of {} is invalid", len).into(),
            ))
        }
    };

    Ok(res)
}
