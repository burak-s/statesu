//! Plain-text table rendering for the default (human) output mode.

/// Renders aligned columns: a header row, a dash separator and data rows.
/// Width is counted in `char`s, a pragmatic fit for the column data here.
pub(crate) fn table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let mut widths: Vec<usize> = headers
        .iter()
        .map(|header| header.chars().count())
        .collect();
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(cell.chars().count());
        }
    }
    let mut out = padded(
        &headers
            .iter()
            .map(|cell| cell.to_string())
            .collect::<Vec<_>>(),
        &widths,
    );
    out.push('\n');
    out.push_str(
        &widths
            .iter()
            .map(|width| "-".repeat(*width))
            .collect::<Vec<_>>()
            .join("  "),
    );
    out.push('\n');
    for row in rows {
        out.push_str(&padded(row, &widths));
        out.push('\n');
    }
    out
}

fn padded(cells: &[String], widths: &[usize]) -> String {
    cells
        .iter()
        .enumerate()
        .map(|(index, text)| format!("{text:<width$}", width = widths[index]))
        .collect::<Vec<_>>()
        .join("  ")
        // Right-hand padding is for columns further out; trim the tail.
        .trim_end()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligns_and_pads_columns() {
        let rendered = table(
            &["slug", "status"],
            &[
                vec!["hello-world".into(), "published".into()],
                vec!["x".into(), "draft".into()],
            ],
        );
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines[0], "slug         status");
        assert_eq!(lines[1], "-----------  ---------");
        assert_eq!(lines[2], "hello-world  published");
        assert_eq!(lines[3], "x            draft");
    }

    #[test]
    fn empty_lists_render_header_only() {
        let rendered = table(&["slug"], &[]);
        assert_eq!(rendered.lines().count(), 2);
    }
}
