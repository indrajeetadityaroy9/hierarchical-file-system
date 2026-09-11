use mathnote::compiler::{CompileRequest, compile_latex};
use mathnote::document::Document;
use mathnote::latex::emit_latex;
use mathnote::preview::{inspect_pdf, rasterize_page};

#[test]
fn embedded_latex_pipeline_handles_blank_and_visible_pages_from_one_verified_cache() {
    let document = Document::parse("").expect("empty note is valid");
    let latex = emit_latex(&document);
    let compiled = compile_latex(CompileRequest::new(0, &latex))
        .expect("empty note compiles with embedded Tectonic");
    let pdf = inspect_pdf(&compiled.pdf).expect("empty document PDF parses with Hayro");

    assert_eq!(pdf.page_count, 1);

    let note = Document::parse(
        "Proof: if $x squared equals 81$, then $root of 81$ equals 9.\n\n$$\nintegral of x\n$$\n",
    )
    .expect("representative mathematics note");
    let latex = emit_latex(&note);
    let compiled =
        compile_latex(CompileRequest::new(42, &latex)).expect("embedded Tectonic compilation");

    assert_eq!(compiled.revision, 42);
    assert!(compiled.pdf.starts_with(b"%PDF-"));
    assert_eq!(
        compiled.bundle_url,
        "https://data1b.fullyjustified.net/tlextras-2022.0r0.tar"
    );
    assert_eq!(
        compiled.bundle_digest,
        "6ffe055852f8faf66c0acbe1a7fb27f87b869a90bad1204f3bf4d9683f597c7c"
    );
    assert!(compiled.diagnostics.messages.iter().all(|message| {
        let message = message.message.to_ascii_lowercase();
        !message.contains("downloading") && !message.contains("network")
    }));

    let info = inspect_pdf(&compiled.pdf).expect("Hayro parses Tectonic PDF");
    assert!(info.page_count >= 1);

    let preview = rasterize_page(&compiled.pdf, 0, 900).expect("Hayro rasterizes first page");
    assert_eq!(preview.width, 900);
    assert!(preview.height > preview.width);

    let rgba = preview.image.to_rgba8();
    let dark_pixels = rgba
        .pixels()
        .filter(|pixel| pixel.0[0] < 230 || pixel.0[1] < 230 || pixel.0[2] < 230)
        .count();
    assert!(
        dark_pixels > 500,
        "rendered page must contain visible text and mathematics"
    );
}
