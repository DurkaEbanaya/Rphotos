//! Opt-in public documentation captures; never reads the user's library.
use super::*;
use egui_kittest::Harness;

#[test]
#[ignore = "requires explicitly supplied licensed photos, RPHOTOS_SHOWCASE_INPUT and RPHOTOS_CAPTURE_DIR"]
fn capture_showcase() {
    let input = PathBuf::from(
        std::env::var_os("RPHOTOS_SHOWCASE_INPUT").expect("set RPHOTOS_SHOWCASE_INPUT"),
    );
    let output =
        PathBuf::from(std::env::var_os("RPHOTOS_CAPTURE_DIR").expect("set RPHOTOS_CAPTURE_DIR"));
    std::fs::create_dir_all(&output).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("Travel journal");
    std::fs::create_dir(&source).unwrap();
    let mut inputs: Vec<_> = std::fs::read_dir(input)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "jpg"))
        .collect();
    inputs.sort();
    assert!(inputs.len() >= 6, "supply at least six licensed JPGs");
    for path in &inputs {
        std::fs::copy(path, source.join(path.file_name().unwrap())).unwrap();
    }
    let database = temp.path().join("data/library.sqlite3");
    let mut db = store::connect(&database).unwrap();
    store::add_source(&db, &source).unwrap();
    store::scan(&mut db, &std::sync::atomic::AtomicU64::new(1), 1, |_| {}).unwrap();
    db.execute("UPDATE photos SET modified=1791028800", [])
        .unwrap();
    // Dates and albums below are demonstration data, not inferred EXIF or AI.
    db.execute("DELETE FROM photo_dates", []).unwrap();
    let state = store::snapshot(&db).unwrap();
    let mut albums = Vec::new();
    for (name, fragment) in [
        ("Тихий берег", "coast"),
        ("Свет в лесу", "forest"),
        ("Красные скалы", "arches"),
        ("За горизонт", "balloon"),
        ("Древний город", "petra"),
    ] {
        let ids: Vec<_> = state
            .photos
            .iter()
            .filter(|p| {
                p.path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(fragment)
            })
            .map(|p| p.id)
            .collect();
        albums.push(store::create_album_with_photos(&mut db, name, &ids).unwrap());
    }
    for p in state.photos.iter().take(3) {
        store::toggle_favorite(&db, p.id).unwrap();
    }
    drop(db);
    for (name, appearance, section, album) in [
        (
            "collection-light",
            Appearance::Light,
            Section::Collection,
            None,
        ),
        (
            "collection-dark",
            Appearance::Dark,
            Section::Collection,
            None,
        ),
        ("albums", Appearance::Light, Section::Albums, None),
        (
            "album-detail",
            Appearance::Dark,
            Section::Albums,
            Some(albums[2]),
        ),
    ] {
        let ctx = egui::Context::default();
        let lib = Library::with_paths(&ctx, database.clone(), temp.path().join("cache"));
        let settings = AppSettings {
            appearance,
            thumbnail_size: 190.0,
            ..Default::default()
        };
        let mut h = Harness::builder()
            .with_size(egui::vec2(
                1600.0,
                if name == "albums" { 720.0 } else { 900.0 },
            ))
            .wgpu()
            .build_state(
                |ctx, (lib, settings)| {
                    lib.poll(ctx);
                    lib.show(ctx, settings);
                },
                (lib, settings),
            );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !h.state().0.ready {
            h.run_steps(2);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        h.state_mut().0.section = section;
        if let Some(id) = album {
            h.state_mut().0.enter_album(id);
        }
        loop {
            h.run_steps(3);
            if h.state().0.thumbnails.pending_count() == 0 {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // Allow batched aspect discovery to settle before the public render.
        h.state_mut().0.layout_key = None;
        h.run_steps(5);
        h.render()
            .unwrap()
            .save(output.join(format!("{name}.png")))
            .unwrap();
    }
    let path = source.join("08-petra.jpg");
    assert!(path.is_file());
    for (name, appearance) in [
        ("viewer-light", Appearance::Light),
        ("viewer-dark", Appearance::Dark),
    ] {
        let ctx = egui::Context::default();
        let mut pane = crate::pane::Pane::new(&ctx, 2, 64, 1, false, true, 0);
        pane.image_paths = vec![path.clone()];
        let loaded = crate::file_io::load_image(&path);
        pane.current_record = Some(loaded.record);
        // kittest begins with a conservative 2048 texture limit; still larger
        // than the fitted image in this 1600px capture. No UI is composited.
        let pixels = crate::decode::image_to_color_image(
            loaded.image.unwrap().thumbnail(2048, 2048),
            loaded.orientation,
        );
        let settings = AppSettings {
            appearance,
            ..Default::default()
        };
        let mut h = Harness::builder()
            .with_size(egui::vec2(1600.0, 1000.0))
            .wgpu()
            .build_state(
                move |ctx, (viewer, pane, settings, uploaded)| {
                    if !*uploaded {
                        pane.current_texture = Some(ctx.load_texture(
                            "showcase-photo",
                            pixels.clone(),
                            egui::TextureOptions::LINEAR,
                        ));
                        *uploaded = true;
                    }
                    viewer.show(ctx, pane, settings, false);
                },
                (crate::viewer::Viewer::default(), pane, settings, false),
            );
        h.run_steps(5);
        h.render()
            .unwrap()
            .save(output.join(format!("{name}.png")))
            .unwrap();
    }
}
