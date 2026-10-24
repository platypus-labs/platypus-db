//! PlatypusDB: `sql_variant` values, read as a value of their base type.

use std::borrow::Cow;

use byteorder::{ByteOrder, LittleEndian};
use uuid::Uuid;

use crate::{
    error::Error,
    sql_read_bytes::SqlReadBytes,
    tds::{codec::guid, Collation, Numeric},
    ColumnData, FixedLenType, VarLenType,
};

#[cfg(feature = "tds73")]
use crate::time::{Date, DateTime2, DateTimeOffset, Time};

/// Reads `len` bytes.
async fn bytes<R>(src: &mut R, len: usize) -> crate::Result<Vec<u8>>
where
    R: SqlReadBytes + Unpin,
{
    let mut buf = vec![0u8; len];
    for b in &mut buf {
        *b = src.read_u8().await?;
    }
    Ok(buf)
}

/// The bytes of a `time` with `scale` fractional digits.
#[cfg(feature = "tds73")]
fn time_len(scale: u8) -> usize {
    match scale {
        0..=2 => 3,
        3..=4 => 4,
        _ => 5,
    }
}

/// A value of a column of type `sql_variant`: a 4-byte length (0 for NULL), the base type,
/// the length of its properties, the properties and the value. A base type this reader
/// does not know is returned as the value's bytes.
pub(crate) async fn decode<R>(src: &mut R) -> crate::Result<ColumnData<'static>>
where
    R: SqlReadBytes + Unpin,
{
    let total = src.read_u32_le().await? as usize;
    if total == 0 {
        return Ok(ColumnData::String(None));
    }
    if total < 2 {
        return Err(Error::Protocol("sql_variant: invalid length".into()));
    }
    let base = src.read_u8().await?;
    let prop_len = src.read_u8().await? as usize;
    let data_len = total
        .checked_sub(2 + prop_len)
        .ok_or_else(|| Error::Protocol("sql_variant: invalid property length".into()))?;
    let props = bytes(src, prop_len).await?;

    if let Ok(ty) = FixedLenType::try_from(base) {
        if prop_len == 0 && ty != FixedLenType::Null {
            return super::fixed_len::decode(src, &ty).await;
        }
    }

    let data = bytes(src, data_len).await?;
    let value = match VarLenType::try_from(base) {
        Ok(VarLenType::Guid) if data.len() == 16 => {
            let mut raw = [0u8; 16];
            raw.copy_from_slice(&data);
            guid::reorder_bytes(&mut raw);
            ColumnData::Guid(Some(Uuid::from_bytes(raw)))
        }
        Ok(VarLenType::Decimaln | VarLenType::Numericn) if props.len() == 2 && !data.is_empty() => {
            let sign = if data[0] == 0 { -1i128 } else { 1i128 };
            let mut magnitude = [0u8; 16];
            let digits = &data[1..];
            if digits.len() > 16 {
                return Err(Error::Protocol("sql_variant: invalid decimal".into()));
            }
            magnitude[..digits.len()].copy_from_slice(digits);
            let value = LittleEndian::read_u128(&magnitude) as i128 * sign;
            ColumnData::Numeric(Some(Numeric::new_with_scale(value, props[1])))
        }
        Ok(VarLenType::BigVarBin | VarLenType::BigBinary) => {
            ColumnData::Binary(Some(Cow::Owned(data)))
        }
        Ok(VarLenType::BigVarChar | VarLenType::BigChar) if props.len() >= 5 => {
            let collation = Collation::new(LittleEndian::read_u32(&props[0..4]), props[4]);
            let text = collation
                .encoding()?
                .decode_without_bom_handling_and_without_replacement(&data)
                .ok_or_else(|| Error::Encoding("invalid sequence".into()))?
                .into_owned();
            ColumnData::String(Some(Cow::Owned(text)))
        }
        Ok(VarLenType::NVarchar | VarLenType::NChar) => {
            if data.len() % 2 != 0 {
                return Err(Error::Protocol("sql_variant: invalid nvarchar".into()));
            }
            let units: Vec<u16> = data.chunks(2).map(LittleEndian::read_u16).collect();
            ColumnData::String(Some(Cow::Owned(String::from_utf16(&units)?)))
        }
        #[cfg(feature = "tds73")]
        Ok(VarLenType::Daten) if data.len() == 3 => {
            ColumnData::Date(Some(Date::new(read_u24(&data))))
        }
        #[cfg(feature = "tds73")]
        Ok(VarLenType::Timen) if props.len() == 1 && data.len() == time_len(props[0]) => {
            ColumnData::Time(Some(Time::new(read_uint(&data), props[0])))
        }
        #[cfg(feature = "tds73")]
        Ok(VarLenType::Datetime2) if props.len() == 1 && data.len() == time_len(props[0]) + 3 => {
            let n = time_len(props[0]);
            let time = Time::new(read_uint(&data[..n]), props[0]);
            let date = Date::new(read_u24(&data[n..]));
            ColumnData::DateTime2(Some(DateTime2::new(date, time)))
        }
        #[cfg(feature = "tds73")]
        Ok(VarLenType::DatetimeOffsetn)
            if props.len() == 1 && data.len() == time_len(props[0]) + 5 =>
        {
            let n = time_len(props[0]);
            let time = Time::new(read_uint(&data[..n]), props[0]);
            let date = Date::new(read_u24(&data[n..n + 3]));
            let offset = LittleEndian::read_i16(&data[n + 3..]);
            ColumnData::DateTimeOffset(Some(DateTimeOffset::new(DateTime2::new(date, time), offset)))
        }
        _ => ColumnData::Binary(Some(Cow::Owned(data))),
    };
    Ok(value)
}

/// A little-endian unsigned number of up to 8 bytes.
#[cfg(feature = "tds73")]
fn read_uint(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .rev()
        .fold(0u64, |acc, &b| (acc << 8) | u64::from(b))
}

#[cfg(feature = "tds73")]
fn read_u24(bytes: &[u8]) -> u32 {
    read_uint(&bytes[..3]) as u32
}
