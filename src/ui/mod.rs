pub mod app;
pub mod theme;
pub mod views;

#[cfg(test)]
pub(crate) fn render(w: u16, h: u16, draw: impl FnOnce(&mut ratatui::Frame)) -> String {
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    t.draw(draw).unwrap();
    let buf = t.backend().buffer();
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}
