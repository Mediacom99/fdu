use anyhow::{Context, Result};
pub fn parse_size(s: &str) -> Result<u64> {
    let s = s.trim();
    anyhow::ensure!(!s.is_empty(), "Empty size string");

    if let Ok(size) = s.parse::<u64>() {
        return Ok(size);
    }

    // Find boundary between number and suffix (e.g. 10<boundary>MB)
    let boundary = s.find(|c: char| c.is_alphabetic()).unwrap_or(s.len());
    anyhow::ensure!(boundary > 0, "No numeric value found");

    let (num_part, suffix) = s.split_at(boundary);

    let multiplier = match suffix.to_uppercase().as_str() {
        "" | "B" => 1,
        "K" | "KB" => 1_024,
        "M" | "MB" => 1_024_u64.pow(2),
        "G" | "GB" => 1_024_u64.pow(3),
        "T" | "TB" => 1_024_u64.pow(4),
        "P" | "PB" => 1_024_u64.pow(5),
        _ => anyhow::bail!("Unknown size suffix: {}", suffix),
    };

    let num: f64 = num_part
        .parse()
        .with_context(|| format!("Invalid size number: {num_part}'"))?;

    anyhow::ensure!(num >= 0.0, "Size cannot be negative");

    let result = (num * multiplier as f64) as u64;

    //TODO: need this ?
    anyhow::ensure!(
        result as f64 / multiplier as f64 - num < 0.01,
        "Size value too large"
    );

    Ok(result)
}
