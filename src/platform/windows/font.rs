//! The typeface from a file of ours. cairo's toy font API looks a family up through DirectWrite's
//! system collection, which never holds a font added at run time (AddFontResourceEx, private or not):
//! asked for one, it draws Arial. So the file is opened with FreeType (linked into cairo already) and
//! the face set directly, which also draws the glyphs as on Linux.

use std::ffi::{c_long, c_void};
use std::path::Path;
use std::sync::Mutex;

unsafe extern "C" {
    fn FT_Init_FreeType(library: *mut *mut c_void) -> i32;
    fn FT_New_Memory_Face(library: *mut c_void, data: *const u8, size: c_long, index: c_long, face: *mut *mut c_void) -> i32;
    fn cairo_ft_font_face_create_for_ft_face(face: *mut c_void, load_flags: i32) -> *mut cairo::ffi::cairo_font_face_t;
    fn cairo_ft_font_face_set_synthesize(face: *mut cairo::ffi::cairo_font_face_t, flags: u32);
}

const FT_STYLE_FLAG_BOLD: c_long = 2;
const CAIRO_FT_SYNTHESIZE_BOLD: u32 = 1;

/// A cairo font face, kept for the life of the process.
#[derive(Clone, Copy)]
struct Face(*mut cairo::ffi::cairo_font_face_t);

// SAFETY: the face is never released, so the pointer stays valid; cairo counts references to it
// atomically and holds its own lock round every use of the FreeType face beneath it.
unsafe impl Send for Face {}

struct Faces {
    library: *mut c_void,
    regular: Option<Face>,
    bold: Option<Face>,
    /// The regular face again, emboldened: the bold where no bold file is registered.
    synthetic: Option<Face>,
}

// SAFETY: FreeType allows one library on several threads when faces are made and freed under a lock:
// `register` makes them under FACES's lock and none is freed; each face is used by cairo, under its
// own lock.
unsafe impl Send for Faces {}

static FACES: Mutex<Faces> = Mutex::new(Faces { library: std::ptr::null_mut(), regular: None, bold: None, synthetic: None });

/// Open the font file as the regular or bold face, as its style says.
pub fn register(path: &Path) -> Result<(), String> {
    let data: &'static [u8] = Box::leak(std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?.into_boxed_slice());
    let mut f = FACES.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: under FACES's lock; FreeType calls with valid out-pointers, each face checked for null;
    // the font data is leaked, so it outlives the faces that read it; FT_FaceRec starts with four
    // FT_Long (c_long): num_faces, face_index, face_flags, style_flags. cairo takes the FreeType faces,
    // which are never freed.
    unsafe {
        if f.library.is_null() && FT_Init_FreeType(&mut f.library) != 0 {
            f.library = std::ptr::null_mut();
            return Err("FreeType did not start".into());
        }
        let mut ft = std::ptr::null_mut();
        if FT_New_Memory_Face(f.library, data.as_ptr(), data.len() as c_long, 0, &mut ft) != 0 || ft.is_null() {
            return Err(format!("{}: not a font FreeType reads", path.display()));
        }
        let bold = *(ft as *const c_long).add(3) & FT_STYLE_FLAG_BOLD != 0;
        let face = Face(cairo_ft_font_face_create_for_ft_face(ft, 0));
        if bold {
            f.bold = Some(face);
        } else {
            // a FreeType face of its own: cairo keeps one face per FreeType face and load flags
            let mut again = std::ptr::null_mut();
            if FT_New_Memory_Face(f.library, data.as_ptr(), data.len() as c_long, 0, &mut again) != 0 || again.is_null() {
                return Err(format!("{}: not a font FreeType reads", path.display()));
            }
            let synthetic = Face(cairo_ft_font_face_create_for_ft_face(again, 0));
            cairo_ft_font_face_set_synthesize(synthetic.0, CAIRO_FT_SYNTHESIZE_BOLD);
            f.regular = Some(face);
            f.synthetic = Some(synthetic);
        }
    }
    Ok(())
}

/// The registered face, bold or not; None before one is registered.
pub fn face(bold: bool) -> Option<cairo::FontFace> {
    let f = FACES.lock().unwrap_or_else(|e| e.into_inner());
    let face = if bold { f.bold.or(f.synthetic) } else { f.regular }?;
    // SAFETY: the face is alive (never released); the reference taken here is the one the FontFace
    // owns and drops.
    Some(unsafe { cairo::FontFace::from_raw_full(cairo::ffi::cairo_font_face_reference(face.0)) })
}
