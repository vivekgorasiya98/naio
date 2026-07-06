//! Chart types and SVG/ASCII rendering.

#[derive(Clone)]
pub struct LineChart {
    pub data: Vec<f32>,
    pub title: String,
}

#[derive(Clone)]
pub struct HistogramChart {
    pub data: Vec<f32>,
    pub bins: usize,
}

#[derive(Clone)]
pub struct ScatterChart {
    pub x: Vec<f32>,
    pub y: Vec<f32>,
}

#[derive(Clone)]
pub struct HeatmapChart {
    pub data: Vec<f32>,
    pub rows: usize,
    pub cols: usize,
}

#[derive(Clone)]
pub struct BarChart {
    pub data: Vec<f32>,
}

#[derive(Clone)]
pub enum Chart {
    Line(LineChart),
    Hist(HistogramChart),
    Scatter(ScatterChart),
    Heatmap(HeatmapChart),
    Bar(BarChart),
}

impl Chart {
    pub fn to_svg(&self) -> String {
        match self {
            Chart::Line(c) => line_svg(c),
            Chart::Hist(c) => hist_svg(c),
            Chart::Scatter(c) => scatter_svg(c),
            Chart::Heatmap(c) => heatmap_svg(c),
            Chart::Bar(c) => bar_svg(c),
        }
    }

    pub fn to_ascii(&self) -> String {
        match self {
            Chart::Line(c) => line_ascii(c),
            Chart::Hist(c) => hist_ascii(c),
            Chart::Scatter(c) => format!("scatter[{} points]", c.x.len().min(c.y.len())),
            Chart::Heatmap(c) => format!("heatmap[{}x{}]", c.rows, c.cols),
            Chart::Bar(c) => bar_ascii(c),
        }
    }

    pub fn to_csv(&self) -> String {
        match self {
            Chart::Line(c) => c.data.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n"),
            Chart::Hist(c) => {
                let counts = bin_counts(&c.data, c.bins);
                counts.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n")
            }
            Chart::Scatter(c) => c
                .x
                .iter()
                .zip(c.y.iter())
                .map(|(a, b)| format!("{a},{b}"))
                .collect::<Vec<_>>()
                .join("\n"),
            Chart::Heatmap(c) => c.data.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n"),
            Chart::Bar(c) => c.data.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n"),
        }
    }
}

fn line_svg(c: &LineChart) -> String {
    let w = 400;
    let h = 200;
    let pad = 20.0;
    if c.data.is_empty() {
        return format!("<svg width=\"{w}\" height=\"{h}\"><text x=\"10\" y=\"20\">{}</text></svg>", c.title);
    }
    let min = c.data.iter().cloned().fold(f32::INFINITY, f32::min);
    let max = c.data.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let range = (max - min).max(1e-6);
    let n = c.data.len();
    let mut pts = String::new();
    for (i, &v) in c.data.iter().enumerate() {
        let x = pad + (i as f32 / (n - 1).max(1) as f32) * (w as f32 - 2.0 * pad);
        let y = h as f32 - pad - ((v - min) / range) * (h as f32 - 2.0 * pad);
        pts.push_str(&format!("{x:.1},{y:.1} "));
    }
    format!(
        "<svg width=\"{w}\" height=\"{h}\" xmlns=\"http://www.w3.org/2000/svg\">\
         <title>{}</title><polyline fill=\"none\" stroke=\"#3b82f6\" stroke-width=\"2\" points=\"{pts}\"/>\
         </svg>",
        c.title
    )
}

fn line_ascii(c: &LineChart) -> String {
    let rows = 12;
    let cols = 48usize.min(c.data.len().max(1));
    if c.data.is_empty() {
        return "(empty)".into();
    }
    let min = c.data.iter().cloned().fold(f32::INFINITY, f32::min);
    let max = c.data.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let range = (max - min).max(1e-6);
    let mut grid = vec![vec![' '; cols]; rows];
    for (i, &v) in c.data.iter().enumerate() {
        let col = i * cols / c.data.len();
        let row = rows - 1 - ((v - min) / range * (rows - 1) as f32) as usize;
        if col < cols && row < rows {
            grid[row][col] = '*';
        }
    }
    let mut out = format!("{}\n", c.title);
    for row in grid {
        out.push('|');
        for ch in row {
            out.push(ch);
        }
        out.push('\n');
    }
    out
}

fn bin_counts(data: &[f32], bins: usize) -> Vec<usize> {
    let bins = bins.max(1);
    if data.is_empty() {
        return vec![0; bins];
    }
    let min = data.iter().cloned().fold(f32::INFINITY, f32::min);
    let max = data.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let range = (max - min).max(1e-6);
    let mut counts = vec![0usize; bins];
    for &v in data {
        let b = (((v - min) / range) * bins as f32) as usize;
        counts[b.min(bins - 1)] += 1;
    }
    counts
}

fn hist_svg(c: &HistogramChart) -> String {
    let w = 400;
    let h = 200;
    let counts = bin_counts(&c.data, c.bins);
    let max_c = counts.iter().max().copied().unwrap_or(1) as f32;
    let bar_w = (w as f32 - 40.0) / c.bins as f32;
    let mut rects = String::new();
    for (i, &cnt) in counts.iter().enumerate() {
        let bh = (cnt as f32 / max_c) * (h as f32 - 40.0);
        let x = 20.0 + i as f32 * bar_w;
        let y = h as f32 - 20.0 - bh;
        rects.push_str(&format!(
            "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{:.1}\" height=\"{bh:.1}\" fill=\"#6366f1\"/>",
            bar_w - 2.0
        ));
    }
    format!("<svg width=\"{w}\" height=\"{h}\" xmlns=\"http://www.w3.org/2000/svg\">{rects}</svg>")
}

fn hist_ascii(c: &HistogramChart) -> String {
    let counts = bin_counts(&c.data, c.bins);
    let max_c = counts.iter().max().copied().unwrap_or(1);
    counts
        .iter()
        .map(|&c| {
            let n = (c as f32 / max_c as f32 * 20.0) as usize;
            "#".repeat(n.max(1))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn scatter_svg(c: &ScatterChart) -> String {
    let w = 400;
    let h = 200;
    let n = c.x.len().min(c.y.len());
    if n == 0 {
        return format!("<svg width=\"{w}\" height=\"{h}\"></svg>");
    }
    let xmin = c.x.iter().take(n).cloned().fold(f32::INFINITY, f32::min);
    let xmax = c.x.iter().take(n).cloned().fold(f32::NEG_INFINITY, f32::max);
    let ymin = c.y.iter().take(n).cloned().fold(f32::INFINITY, f32::min);
    let ymax = c.y.iter().take(n).cloned().fold(f32::NEG_INFINITY, f32::max);
    let mut circles = String::new();
    for i in 0..n {
        let x = 20.0 + (c.x[i] - xmin) / (xmax - xmin).max(1e-6) * (w as f32 - 40.0);
        let y = h as f32 - 20.0 - (c.y[i] - ymin) / (ymax - ymin).max(1e-6) * (h as f32 - 40.0);
        circles.push_str(&format!("<circle cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"2\" fill=\"#ef4444\"/>"));
    }
    format!("<svg width=\"{w}\" height=\"{h}\" xmlns=\"http://www.w3.org/2000/svg\">{circles}</svg>")
}

fn heatmap_svg(c: &HeatmapChart) -> String {
    let cell = 12;
    let w = c.cols * cell;
    let h = c.rows * cell;
    let max = c.data.iter().cloned().fold(0.0f32, f32::max).max(1e-6);
    let mut rects = String::new();
    for r in 0..c.rows {
        for col in 0..c.cols {
            let v = c.data.get(r * c.cols + col).copied().unwrap_or(0.0);
            let intensity = (v / max * 255.0) as u8;
            rects.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{cell}\" height=\"{cell}\" fill=\"rgb({},{},{})\"/>",
                col * cell,
                r * cell,
                intensity,
                intensity / 2,
                255 - intensity / 2
            ));
        }
    }
    format!("<svg width=\"{w}\" height=\"{h}\" xmlns=\"http://www.w3.org/2000/svg\">{rects}</svg>")
}

fn bar_svg(c: &BarChart) -> String {
    let chart = HistogramChart {
        data: c.data.clone(),
        bins: c.data.len().max(1),
    };
    hist_svg(&chart)
}

fn bar_ascii(c: &BarChart) -> String {
    let max = c.data.iter().cloned().fold(0.0f32, f32::max).max(1.0);
    c.data
        .iter()
        .enumerate()
        .map(|(i, &v)| format!("{i}: {}", "#".repeat((v / max * 20.0) as usize + 1)))
        .collect::<Vec<_>>()
        .join("\n")
}
