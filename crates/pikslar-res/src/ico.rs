//! Reads a `.ico` file and turns it into `RT_ICON` images plus an `RT_GROUP_ICON` directory.
//!
//! An `.ico` file is a 6-byte header, 16-byte directory entries and the images. In a program
//! file each image becomes its own `RT_ICON` resource, and the group directory repeats each
//! entry with its last field (the image's file offset) replaced by the image's resource id.

use crate::bytes::{get16, get32, put16, slice};
use crate::{Resource, RT_GROUP_ICON, RT_ICON};

/// One image listed in an icon file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// The first 12 bytes of the directory entry: size, colours, planes, bits and length.
    pub entry: [u8; 12],
    pub data: Vec<u8>,
}

/// Reads the images of an `.ico` file.
pub fn parse(ico: &[u8]) -> Result<Vec<Image>, String> {
    if get16(ico, 0)? != 0 || get16(ico, 2)? != 1 {
        return Err("not an icon file (expected reserved 0, type 1)".into());
    }
    let count = get16(ico, 4)? as usize;
    if count == 0 {
        return Err("the icon file has no images".into());
    }
    (0..count)
        .map(|i| {
            let at = 6 + 16 * i;
            let mut entry = [0; 12];
            entry.copy_from_slice(slice(ico, at, 12)?);
            let (len, offset) = (get32(ico, at + 8)? as usize, get32(ico, at + 12)? as usize);
            let data = slice(ico, offset, len).map_err(|e| format!("icon image {i}: {e}"))?;
            Ok(Image {
                entry,
                data: data.to_vec(),
            })
        })
        .collect()
}

/// The icon's resources: images with ids `first_id..`, and the group as `group_id`.
pub fn resources(
    ico: &[u8],
    group_id: u16,
    first_id: u16,
    lang: u16,
) -> Result<Vec<Resource>, String> {
    let images = parse(ico)?;
    let count = u16::try_from(images.len()).map_err(|_| "too many icon images")?;
    first_id
        .checked_add(count - 1)
        .ok_or("icon image ids would run past 65535")?;
    let mut group = vec![0, 0, 1, 0];
    put16(&mut group, count);
    let mut out = Vec::with_capacity(images.len() + 1);
    for (id, image) in (first_id..=u16::MAX).zip(images) {
        group.extend_from_slice(&image.entry);
        put16(&mut group, id);
        out.push(Resource {
            ty: RT_ICON,
            id,
            lang,
            data: image.data,
        });
    }
    out.push(Resource {
        ty: RT_GROUP_ICON,
        id: group_id,
        lang,
        data: group,
    });
    Ok(out)
}

#[cfg(test)]
#[path = "ico_tests.rs"]
mod tests;
