//! Toolkit-independent justified rows. Last rows never stretch to fill a month.
#[derive(Clone, Debug)]
pub(super) struct Cell {
    pub index: usize,
    pub x: f32,
    pub width: f32,
}

#[derive(Clone, Debug)]
pub(super) struct Row {
    pub cells: Vec<Cell>,
    pub height: f32,
}

pub(super) fn rows(items: &[(usize, f32)], width: f32, target: f32, gap: f32) -> Vec<Row> {
    let width = width.max(1.0);
    let target = target.max(1.0).min(width);
    let mut result = Vec::new();
    let mut start = 0;
    while start < items.len() {
        let mut end = start;
        let mut sum = 0.0;
        while end < items.len() {
            if end > start && gap * (end - start) as f32 >= width {
                break;
            }
            let ratio = aspect(items[end].1);
            let next = sum + ratio;
            let occupied = next * target + gap * (end - start) as f32;
            if occupied >= width {
                // Pick the row break whose height is closest to the target.
                if end > start {
                    let without = (width - gap * (end - start - 1) as f32) / sum;
                    let with = (width - gap * (end - start) as f32) / next;
                    if (without - target).abs() < (with - target).abs() {
                        break;
                    }
                }
                sum = next;
                end += 1;
                break;
            }
            sum = next;
            end += 1;
        }
        let count = end - start;
        let fit = ((width - gap * count.saturating_sub(1) as f32) / sum).max(1.0);
        let height = if end == items.len() {
            target.min(fit)
        } else {
            fit
        };
        let mut x = 0.0;
        let mut cells = Vec::with_capacity(count);
        for &(index, ratio) in &items[start..end] {
            let w = (aspect(ratio) * height).min((width - x).max(1.0));
            cells.push(Cell { index, x, width: w });
            x += w + gap;
        }
        result.push(Row { cells, height });
        start = end;
    }
    result
}

fn aspect(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value.clamp(0.2, 5.0)
    } else {
        1.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_rows_keep_order_bounds_and_last_row_height() {
        for width in [100.0, 468.0, 1260.0] {
            for target in [96.0, 256.0, 512.0] {
                let items: Vec<_> = (0..53)
                    .map(|i| (i, [1.5, 2.0 / 3.0, 1.0, 3.0, 0.25][i % 5]))
                    .collect();
                let rows = rows(&items, width, target, 12.0);
                assert_eq!(
                    rows.iter()
                        .flat_map(|r| r.cells.iter().map(|c| c.index))
                        .collect::<Vec<_>>(),
                    (0..53).collect::<Vec<_>>()
                );
                for r in &rows {
                    assert!(r.height.is_finite() && r.height > 0.0);
                    let mut previous = 0.0;
                    for cell in &r.cells {
                        assert!(cell.width > 0.0 && cell.x >= previous);
                        assert!(cell.x + cell.width <= width + 0.01);
                        previous = cell.x + cell.width;
                    }
                }
                assert!(rows.last().unwrap().height <= target);
            }
        }
    }

    #[test]
    fn portrait_landscape_and_invalid_aspects_are_safe() {
        let r = rows(&[(0, 1.5), (1, 2.0 / 3.0)], 1260.0, 256.0, 12.0);
        assert_eq!(r.len(), 1);
        assert!((r[0].cells[0].width / 256.0 - 1.5).abs() < 0.001);
        assert!((r[0].cells[1].width / 256.0 - 2.0 / 3.0).abs() < 0.001);
        assert!(rows(&[], 500.0, 256.0, 12.0).is_empty());
        for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let r = rows(&[(0, bad)], 1.0, 512.0, 12.0);
            assert!(r[0].cells[0].width.is_finite());
        }
    }
}
