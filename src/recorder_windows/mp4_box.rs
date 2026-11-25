#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxHeader {
    pub fourcc: [u8; 4],
    pub total_size: usize,
    pub header_size: usize,
}

pub fn parse_box_header(data: &[u8]) -> Option<BoxHeader> {
    if data.len() < 8 {
        return None;
    }
    let size32 = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let fourcc: [u8; 4] = [data[4], data[5], data[6], data[7]];
    if size32 == 1 {
        if data.len() < 16 {
            return None;
        }
        let size64 = u64::from_be_bytes(data[8..16].try_into().ok()?) as usize;
        if size64 < 16 || data.len() < size64 {
            return None;
        }
        Some(BoxHeader {
            fourcc,
            total_size: size64,
            header_size: 16,
        })
    } else if size32 == 0 {
        Some(BoxHeader {
            fourcc,
            total_size: data.len(),
            header_size: 8,
        })
    } else if size32 >= 8 {
        if data.len() < size32 {
            return None;
        }
        Some(BoxHeader {
            fourcc,
            total_size: size32,
            header_size: 8,
        })
    } else {
        None
    }
}

pub fn emit_box(fourcc: [u8; 4], payload: &[u8]) -> Vec<u8> {
    let total_size = payload.len() + 8;
    if total_size <= u32::MAX as usize {
        let mut out = Vec::with_capacity(total_size);
        out.extend_from_slice(&(total_size as u32).to_be_bytes());
        out.extend_from_slice(&fourcc);
        out.extend_from_slice(payload);
        out
    } else {
        let total_size_large = payload.len() + 16;
        let mut out = Vec::with_capacity(total_size_large);
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&fourcc);
        out.extend_from_slice(&(total_size_large as u64).to_be_bytes());
        out.extend_from_slice(payload);
        out
    }
}

struct ParsedBox<'a> {
    fourcc: [u8; 4],
    data: &'a [u8],
    payload: &'a [u8],
}

fn parse_children(mut data: &[u8]) -> Option<Vec<ParsedBox<'_>>> {
    let mut boxes = Vec::new();
    while !data.is_empty() {
        let header = parse_box_header(data)?;
        let total = header.total_size;
        let box_data = &data[..total];
        let payload = &data[header.header_size..total];
        boxes.push(ParsedBox {
            fourcc: header.fourcc,
            data: box_data,
            payload,
        });
        data = &data[total..];
    }
    Some(boxes)
}

fn is_video_trak(trak_payload: &[u8]) -> bool {
    let children = match parse_children(trak_payload) {
        Some(c) => c,
        None => return false,
    };
    for b in children {
        if b.fourcc == *b"mdia" {
            if let Some(mdia_children) = parse_children(b.payload) {
                for mb in mdia_children {
                    if mb.fourcc == *b"hdlr" {
                        if mb.payload.len() >= 12 && &mb.payload[8..12] == b"vide" {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

pub fn video_stsd(file: &[u8]) -> Option<Vec<u8>> {
    let mut data = file;
    while !data.is_empty() {
        let header = parse_box_header(data)?;
        let total = header.total_size;
        if header.fourcc == *b"moov" {
            let moov_payload = &data[header.header_size..total];
            return find_video_stsd_in_moov(moov_payload);
        }
        data = &data[total..];
    }
    None
}

fn find_video_stsd_in_moov(moov_payload: &[u8]) -> Option<Vec<u8>> {
    let moov_children = parse_children(moov_payload)?;
    for trak in moov_children {
        if trak.fourcc == *b"trak" && is_video_trak(trak.payload) {
            let trak_children = parse_children(trak.payload)?;
            for mdia in trak_children {
                if mdia.fourcc == *b"mdia" {
                    let mdia_children = parse_children(mdia.payload)?;
                    for minf in mdia_children {
                        if minf.fourcc == *b"minf" {
                            let minf_children = parse_children(minf.payload)?;
                            for stbl in minf_children {
                                if stbl.fourcc == *b"stbl" {
                                    let stbl_children = parse_children(stbl.payload)?;
                                    for stsd in stbl_children {
                                        if stsd.fourcc == *b"stsd" {
                                            return Some(stsd.data.to_vec());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

fn rebuild_stbl(stbl_payload: &[u8], new_stsd: &[u8]) -> Option<Vec<u8>> {
    let children = parse_children(stbl_payload)?;
    let mut new_payload = Vec::new();
    let mut replaced = false;
    for b in children {
        if b.fourcc == *b"stsd" {
            new_payload.extend_from_slice(new_stsd);
            replaced = true;
        } else {
            new_payload.extend_from_slice(b.data);
        }
    }
    if !replaced {
        return None;
    }
    Some(emit_box(*b"stbl", &new_payload))
}

fn rebuild_minf(minf_payload: &[u8], new_stsd: &[u8]) -> Option<Vec<u8>> {
    let children = parse_children(minf_payload)?;
    let mut new_payload = Vec::new();
    let mut replaced = false;
    for b in children {
        if b.fourcc == *b"stbl" {
            let new_stbl = rebuild_stbl(b.payload, new_stsd)?;
            new_payload.extend_from_slice(&new_stbl);
            replaced = true;
        } else {
            new_payload.extend_from_slice(b.data);
        }
    }
    if !replaced {
        return None;
    }
    Some(emit_box(*b"minf", &new_payload))
}

fn rebuild_mdia(mdia_payload: &[u8], new_stsd: &[u8]) -> Option<Vec<u8>> {
    let children = parse_children(mdia_payload)?;
    let mut new_payload = Vec::new();
    let mut replaced = false;
    for b in children {
        if b.fourcc == *b"minf" {
            let new_minf = rebuild_minf(b.payload, new_stsd)?;
            new_payload.extend_from_slice(&new_minf);
            replaced = true;
        } else {
            new_payload.extend_from_slice(b.data);
        }
    }
    if !replaced {
        return None;
    }
    Some(emit_box(*b"mdia", &new_payload))
}

fn rebuild_trak(trak_payload: &[u8], new_stsd: &[u8]) -> Option<Vec<u8>> {
    let children = parse_children(trak_payload)?;
    let mut new_payload = Vec::new();
    let mut replaced = false;
    for b in children {
        if b.fourcc == *b"mdia" {
            let new_mdia = rebuild_mdia(b.payload, new_stsd)?;
            new_payload.extend_from_slice(&new_mdia);
            replaced = true;
        } else {
            new_payload.extend_from_slice(b.data);
        }
    }
    if !replaced {
        return None;
    }
    Some(emit_box(*b"trak", &new_payload))
}

pub fn replace_video_stsd(moov: &[u8], stsd: &[u8]) -> Option<Vec<u8>> {
    let header = parse_box_header(moov)?;
    if header.fourcc != *b"moov" {
        return None;
    }
    let moov_payload = &moov[header.header_size..header.total_size];
    let children = parse_children(moov_payload)?;
    let mut new_payload = Vec::new();
    let mut replaced = false;
    for b in children {
        if b.fourcc == *b"trak" && is_video_trak(b.payload) {
            let new_trak = rebuild_trak(b.payload, stsd)?;
            new_payload.extend_from_slice(&new_trak);
            replaced = true;
        } else {
            new_payload.extend_from_slice(b.data);
        }
    }
    if !replaced {
        return None;
    }
    Some(emit_box(*b"moov", &new_payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_trak(handler_type: &[u8; 4], stsd_bytes: &[u8]) -> Vec<u8> {
        let tkhd = emit_box(*b"tkhd", b"dummy_tkhd_payload");
        let mut hdlr_payload = Vec::new();
        hdlr_payload.extend_from_slice(&[0, 0, 0, 0]); // version/flags
        hdlr_payload.extend_from_slice(&[0, 0, 0, 0]); // pre_defined
        hdlr_payload.extend_from_slice(handler_type);  // handler_type at offset 8
        hdlr_payload.extend_from_slice(b"dummy_name");
        let hdlr = emit_box(*b"hdlr", &hdlr_payload);

        let stbl = emit_box(*b"stbl", stsd_bytes);
        let minf = emit_box(*b"minf", &stbl);

        let mut mdia_payload = Vec::new();
        mdia_payload.extend_from_slice(&hdlr);
        mdia_payload.extend_from_slice(&minf);
        let mdia = emit_box(*b"mdia", &mdia_payload);

        let mut trak_payload = Vec::new();
        trak_payload.extend_from_slice(&tkhd);
        trak_payload.extend_from_slice(&mdia);
        emit_box(*b"trak", &trak_payload)
    }

    #[test]
    fn test_video_stsd_and_replace() {
        let orig_video_stsd = emit_box(*b"stsd", b"initial_video_stsd_bytes");
        let orig_soun_stsd = emit_box(*b"stsd", b"initial_audio_stsd_bytes");

        let video_trak = make_trak(b"vide", &orig_video_stsd);
        let soun_trak = make_trak(b"soun", &orig_soun_stsd);

        let mut moov_payload = Vec::new();
        moov_payload.extend_from_slice(&video_trak);
        moov_payload.extend_from_slice(&soun_trak);
        let moov = emit_box(*b"moov", &moov_payload);

        // Put in a synthetic file with ftyp
        let ftyp = emit_box(*b"ftyp", b"isom\0\0\0\x01isommp42");
        let mut file_data = Vec::new();
        file_data.extend_from_slice(&ftyp);
        file_data.extend_from_slice(&moov);

        // Test video_stsd finds the right stsd
        let found = video_stsd(&file_data).expect("video_stsd should find stsd");
        assert_eq!(found, orig_video_stsd);

        // Test replace_video_stsd with a longer stsd
        let longer_stsd = emit_box(
            *b"stsd",
            b"longer_replacement_video_stsd_with_hevc_parameters_and_more_data_1234567890",
        );
        let delta = longer_stsd.len() as isize - orig_video_stsd.len() as isize;
        assert!(delta > 0);

        let new_moov = replace_video_stsd(&moov, &longer_stsd)
            .expect("replace_video_stsd should succeed");

        // Verify moov size rewritten correctly
        assert_eq!(new_moov.len(), (moov.len() as isize + delta) as usize);
        let moov_hdr = parse_box_header(&new_moov).unwrap();
        assert_eq!(moov_hdr.total_size, new_moov.len());

        // Parse children of new_moov
        let moov_children = parse_children(&new_moov[8..]).unwrap();
        assert_eq!(moov_children.len(), 2);

        // Check new video trak sizes at each level
        let new_video_trak = &moov_children[0];
        assert_eq!(new_video_trak.fourcc, *b"trak");
        assert_eq!(
            new_video_trak.data.len(),
            (video_trak.len() as isize + delta) as usize
        );
        let trak_hdr = parse_box_header(new_video_trak.data).unwrap();
        assert_eq!(
            trak_hdr.total_size,
            (video_trak.len() as isize + delta) as usize
        );

        let trak_children = parse_children(new_video_trak.payload).unwrap();
        let mdia = trak_children
            .iter()
            .find(|b| b.fourcc == *b"mdia")
            .unwrap();
        let mdia_hdr = parse_box_header(mdia.data).unwrap();
        assert_eq!(mdia_hdr.total_size, mdia.data.len());

        let mdia_children = parse_children(mdia.payload).unwrap();
        let minf = mdia_children
            .iter()
            .find(|b| b.fourcc == *b"minf")
            .unwrap();
        let minf_hdr = parse_box_header(minf.data).unwrap();
        assert_eq!(minf_hdr.total_size, minf.data.len());

        let minf_children = parse_children(minf.payload).unwrap();
        let stbl = minf_children
            .iter()
            .find(|b| b.fourcc == *b"stbl")
            .unwrap();
        let stbl_hdr = parse_box_header(stbl.data).unwrap();
        assert_eq!(stbl_hdr.total_size, stbl.data.len());
        let stbl_children = parse_children(stbl.payload).unwrap();
        let stsd = stbl_children
            .iter()
            .find(|b| b.fourcc == *b"stsd")
            .unwrap();

        assert_eq!(stsd.data, longer_stsd.as_slice());

        // Check second 'soun' trak is completely unchanged
        let new_soun_trak = &moov_children[1];
        assert_eq!(new_soun_trak.fourcc, *b"trak");
        assert_eq!(new_soun_trak.data, soun_trak.as_slice());

        // Check video_stsd on replaced moov returns the new stsd
        let found_new = video_stsd(&new_moov).expect("video_stsd should find new stsd");
        assert_eq!(found_new, longer_stsd);
    }

    #[test]
    fn test_largesize_box() {
        let mut large_box = Vec::new();
        large_box.extend_from_slice(&1u32.to_be_bytes()); // size == 1
        large_box.extend_from_slice(b"mdat");
        let total_size: u64 = 32; // 16 bytes header + 16 bytes payload
        large_box.extend_from_slice(&total_size.to_be_bytes());
        large_box.extend_from_slice(&[42u8; 16]); // payload

        let parsed = parse_box_header(&large_box).expect("should parse largesize box");
        assert_eq!(parsed.fourcc, *b"mdat");
        assert_eq!(parsed.total_size, 32);
        assert_eq!(parsed.header_size, 16);

        // Prepend largesize mdat before moov in a file
        let orig_video_stsd = emit_box(*b"stsd", b"video_stsd_for_largesize_test");
        let video_trak = make_trak(b"vide", &orig_video_stsd);
        let moov = emit_box(*b"moov", &video_trak);

        let mut file_data = large_box;
        file_data.extend_from_slice(&moov);

        let found = video_stsd(&file_data).expect("video_stsd should walk past largesize box");
        assert_eq!(found, orig_video_stsd);
    }
}
