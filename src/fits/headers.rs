use anyhow::{Context, Result};
use std::path::Path;

/// Read all header records from `hdu_idx` by parsing the raw FITS file.
///
/// FITS headers consist of 80-byte ASCII records packed into 2880-byte blocks.
/// Each record is `KEY     = value / comment` or a commentary card (COMMENT,
/// HISTORY, blank).  We skip structural/commentary cards and return the rest
/// sorted alphabetically by key name.
pub(super) fn read_headers(fits_path: &Path, hdu_idx: usize) -> Result<Vec<(String, String)>> {
    use std::io::{BufReader, Read, Seek, SeekFrom};

    let file = std::fs::File::open(fits_path)
        .with_context(|| format!("opening {} for header read", fits_path.display()))?;
    let mut reader = BufReader::new(file);

    // Skip over preceding HDUs (each HDU = header blocks + data blocks).
    // For HDU 0 we start at byte 0.
    let mut block = [0u8; 2880];
    let mut hdus_seen = 0usize;

    loop {
        // --- Read header blocks for the current HDU ---
        let mut header_bytes: Vec<u8> = Vec::new();
        let mut found_end = false;
        while !found_end {
            reader.read_exact(&mut block)
                .context("reading FITS header block")?;
            header_bytes.extend_from_slice(&block);
            // Scan this block for an END record
            for rec in block.chunks_exact(80) {
                if rec.starts_with(b"END     ") || rec.starts_with(b"END\x20\x20") || rec == b"END                                                                             " {
                    found_end = true;
                    break;
                }
            }
        }

        if hdus_seen == hdu_idx {
            return Ok(parse_header_records(&header_bytes));
        }

        hdus_seen += 1;

        // Skip the data blocks for this HDU.
        // Data size comes from NAXIS + NAXISn + BITPIX keywords.
        let bitpix = find_header_int(&header_bytes, "BITPIX").unwrap_or(8);
        let naxis = find_header_int(&header_bytes, "NAXIS").unwrap_or(0);
        let mut data_size: u64 = if naxis == 0 {
            0
        } else {
            let bits_per_element = bitpix.unsigned_abs() as u64;
            let mut npix: u64 = 1;
            for i in 1..=naxis {
                let key = format!("NAXIS{i}");
                npix *= find_header_int(&header_bytes, &key).unwrap_or(0).max(0) as u64;
            }
            (npix * bits_per_element + 7) / 8
        };
        // Round up to next 2880-byte boundary
        if data_size % 2880 != 0 {
            data_size += 2880 - data_size % 2880;
        }
        if data_size > 0 {
            reader.seek(SeekFrom::Current(data_size as i64))
                .context("seeking past FITS data block")?;
        }
    }
}

fn parse_header_records(header_bytes: &[u8]) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = Vec::new();
    for rec in header_bytes.chunks_exact(80) {
        let card = std::str::from_utf8(rec).unwrap_or("").trim_end();
        if card.len() < 8 { continue; }
        let key = card[..8].trim().to_string();
        if key.is_empty() || key == "COMMENT" || key == "HISTORY"
            || key == "END" || key == "CONTINUE"
        {
            continue;
        }
        let value = if card.len() > 10 && &card[8..10] == "= " {
            let val_str = strip_fits_comment(card[10..].trim()).trim();
            if val_str.starts_with('\'') && val_str.ends_with('\'') && val_str.len() >= 2 {
                val_str[1..val_str.len() - 1].replace("''", "'").trim().to_string()
            } else {
                val_str.to_string()
            }
        } else if card.len() > 8 {
            card[8..].trim().to_string()
        } else {
            String::new()
        };
        headers.push((key, value));
    }
    headers.sort_by(|a, b| a.0.cmp(&b.0));
    headers
}

/// Remove the ` / comment` part from a FITS value field, respecting quoted strings.
fn strip_fits_comment(s: &str) -> &str {
    let s = s.trim();
    if s.starts_with('\'') {
        // Quoted string — find closing quote (doubled quotes are escaped)
        let mut i = 1;
        let bytes = s.as_bytes();
        while i < bytes.len() {
            if bytes[i] == b'\'' {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                    i += 2; // escaped quote
                } else {
                    return &s[..=i];
                }
            } else {
                i += 1;
            }
        }
        s // no closing quote found, return as-is
    } else {
        // Unquoted — split at first ' / '
        if let Some(pos) = s.find(" / ") {
            s[..pos].trim_end()
        } else {
            s
        }
    }
}

/// Extract an integer value from raw 80-byte FITS header records by keyword name.
pub(super) fn find_header_int(header_bytes: &[u8], key: &str) -> Option<i64> {
    let key_padded = format!("{key:<8}");
    for rec in header_bytes.chunks_exact(80) {
        if rec.starts_with(key_padded.as_bytes()) {
            let card = std::str::from_utf8(rec).ok()?;
            if card.len() > 10 && &card[8..10] == "= " {
                let val = strip_fits_comment(card[10..].trim());
                return val.trim().parse::<i64>().ok();
            }
        }
    }
    None
}
