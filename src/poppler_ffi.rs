//! Fast in-process PDF metadata and Cairo RGBA rasterization via runtime `dlopen` of
//! `libpoppler-glib.so.8` + `libcairo.so.2`.
//!
//! Loading at runtime avoids any compile-time or package-manager hard dependency on `poppler-glib`
//! / `cairo` / `libpoppler.so.<N>` (preventing rolling-release `poppler` conflicts on Arch Linux),
//! while transparently falling back to `pdftocairo`/`pdftoppm` when those libraries are absent.

use crate::compiler::{RenderedPage, RenderedPageData};
use std::ffi::{c_char, c_double, c_int, c_void, CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr;
use std::sync::OnceLock;

const CAIRO_FORMAT_ARGB32: c_int = 0;
const RTLD_LAZY: c_int = 1;

#[repr(C)]
struct PopplerDocument {
    _private: [u8; 0],
}

#[repr(C)]
struct PopplerPage {
    _private: [u8; 0],
}

#[repr(C)]
struct CairoSurface {
    _private: [u8; 0],
}

#[repr(C)]
struct CairoContext {
    _private: [u8; 0],
}

#[repr(C)]
struct GError {
    domain: u32,
    code: c_int,
    message: *const c_char,
}

extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

struct PopplerApi {
    g_filename_to_uri:
        unsafe extern "C" fn(*const c_char, *const c_char, *mut *mut GError) -> *mut c_char,
    g_free: unsafe extern "C" fn(*mut c_void),
    g_error_free: unsafe extern "C" fn(*mut GError),
    g_object_unref: unsafe extern "C" fn(*mut c_void),

    poppler_document_new_from_file:
        unsafe extern "C" fn(*const c_char, *const c_char, *mut *mut GError) -> *mut PopplerDocument,
    poppler_document_get_n_pages: unsafe extern "C" fn(*mut PopplerDocument) -> c_int,
    poppler_document_get_page: unsafe extern "C" fn(*mut PopplerDocument, c_int) -> *mut PopplerPage,
    poppler_page_get_size: unsafe extern "C" fn(*mut PopplerPage, *mut c_double, *mut c_double),
    poppler_page_render: unsafe extern "C" fn(*mut PopplerPage, *mut CairoContext),

    cairo_image_surface_create: unsafe extern "C" fn(c_int, c_int, c_int) -> *mut CairoSurface,
    cairo_image_surface_get_data: unsafe extern "C" fn(*mut CairoSurface) -> *mut u8,
    cairo_image_surface_get_stride: unsafe extern "C" fn(*mut CairoSurface) -> c_int,
    cairo_surface_flush: unsafe extern "C" fn(*mut CairoSurface),
    cairo_surface_destroy: unsafe extern "C" fn(*mut CairoSurface),

    cairo_create: unsafe extern "C" fn(*mut CairoSurface) -> *mut CairoContext,
    cairo_destroy: unsafe extern "C" fn(*mut CairoContext),
    cairo_save: unsafe extern "C" fn(*mut CairoContext),
    cairo_restore: unsafe extern "C" fn(*mut CairoContext),
    cairo_set_source_rgb: unsafe extern "C" fn(*mut CairoContext, c_double, c_double, c_double),
    cairo_paint: unsafe extern "C" fn(*mut CairoContext),
    cairo_scale: unsafe extern "C" fn(*mut CairoContext, c_double, c_double),
}

// All fields are immutable C function pointers loaded once into a process-wide OnceLock.
unsafe impl Send for PopplerApi {}
unsafe impl Sync for PopplerApi {}

unsafe fn open_lib(names: &[&CStr]) -> Result<*mut c_void, String> {
    for name in names {
        let handle = dlopen(name.as_ptr(), RTLD_LAZY);
        if !handle.is_null() {
            return Ok(handle);
        }
    }
    Err(format!(
        "Libreria non disponibile: {}",
        names[0].to_string_lossy()
    ))
}

unsafe fn load_sym<T: Copy>(lib: *mut c_void, name: &CStr) -> Result<T, String> {
    let ptr = dlsym(lib, name.as_ptr());
    if ptr.is_null() {
        return Err(format!("Simbolo mancante: {}", name.to_string_lossy()));
    }
    debug_assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<*mut c_void>());
    Ok(std::mem::transmute_copy(&ptr))
}

impl PopplerApi {
    fn get() -> Result<&'static Self, String> {
        static API: OnceLock<Result<PopplerApi, String>> = OnceLock::new();
        API.get_or_init(|| unsafe { Self::load() })
            .as_ref()
            .map_err(|e| e.clone())
    }

    unsafe fn load() -> Result<Self, String> {
        let glib = open_lib(&[c"libglib-2.0.so.0", c"libglib-2.0.so"])?;
        let gobject = open_lib(&[c"libgobject-2.0.so.0", c"libgobject-2.0.so"])?;
        let poppler = open_lib(&[c"libpoppler-glib.so.8", c"libpoppler-glib.so"])?;
        let cairo = open_lib(&[c"libcairo.so.2", c"libcairo.so"])?;

        Ok(Self {
            g_filename_to_uri: load_sym(glib, c"g_filename_to_uri")?,
            g_free: load_sym(glib, c"g_free")?,
            g_error_free: load_sym(glib, c"g_error_free")?,
            g_object_unref: load_sym(gobject, c"g_object_unref")?,

            poppler_document_new_from_file: load_sym(poppler, c"poppler_document_new_from_file")?,
            poppler_document_get_n_pages: load_sym(poppler, c"poppler_document_get_n_pages")?,
            poppler_document_get_page: load_sym(poppler, c"poppler_document_get_page")?,
            poppler_page_get_size: load_sym(poppler, c"poppler_page_get_size")?,
            poppler_page_render: load_sym(poppler, c"poppler_page_render")?,

            cairo_image_surface_create: load_sym(cairo, c"cairo_image_surface_create")?,
            cairo_image_surface_get_data: load_sym(cairo, c"cairo_image_surface_get_data")?,
            cairo_image_surface_get_stride: load_sym(cairo, c"cairo_image_surface_get_stride")?,
            cairo_surface_flush: load_sym(cairo, c"cairo_surface_flush")?,
            cairo_surface_destroy: load_sym(cairo, c"cairo_surface_destroy")?,

            cairo_create: load_sym(cairo, c"cairo_create")?,
            cairo_destroy: load_sym(cairo, c"cairo_destroy")?,
            cairo_save: load_sym(cairo, c"cairo_save")?,
            cairo_restore: load_sym(cairo, c"cairo_restore")?,
            cairo_set_source_rgb: load_sym(cairo, c"cairo_set_source_rgb")?,
            cairo_paint: load_sym(cairo, c"cairo_paint")?,
            cairo_scale: load_sym(cairo, c"cairo_scale")?,
        })
    }
}

#[cfg(test)]
pub fn is_available() -> bool {
    PopplerApi::get().is_ok()
}

struct PopplerDocGuard(&'static PopplerApi, *mut PopplerDocument);
impl Drop for PopplerDocGuard {
    fn drop(&mut self) {
        if !self.1.is_null() {
            unsafe { (self.0.g_object_unref)(self.1 as *mut c_void) };
        }
    }
}

struct PopplerPageGuard(&'static PopplerApi, *mut PopplerPage);
impl Drop for PopplerPageGuard {
    fn drop(&mut self) {
        if !self.1.is_null() {
            unsafe { (self.0.g_object_unref)(self.1 as *mut c_void) };
        }
    }
}

struct CairoSurfaceGuard(&'static PopplerApi, *mut CairoSurface);
impl Drop for CairoSurfaceGuard {
    fn drop(&mut self) {
        if !self.1.is_null() {
            unsafe { (self.0.cairo_surface_destroy)(self.1) };
        }
    }
}

struct CairoCtxGuard(&'static PopplerApi, *mut CairoContext);
impl Drop for CairoCtxGuard {
    fn drop(&mut self) {
        if !self.1.is_null() {
            unsafe { (self.0.cairo_destroy)(self.1) };
        }
    }
}

fn take_gerror(api: &PopplerApi, err: *mut GError) -> String {
    if err.is_null() {
        return "errore sconosciuto Poppler/GLib".to_string();
    }
    let msg = unsafe {
        if (*err).message.is_null() {
            "errore Poppler/GLib".to_string()
        } else {
            CStr::from_ptr((*err).message).to_string_lossy().into_owned()
        }
    };
    unsafe { (api.g_error_free)(err) };
    msg
}

fn open_document(api: &'static PopplerApi, pdf_path: &Path) -> Result<PopplerDocGuard, String> {
    let abs_path = if pdf_path.is_absolute() {
        pdf_path.to_path_buf()
    } else {
        std::fs::canonicalize(pdf_path).or_else(|_| {
            std::env::current_dir()
                .map(|cwd| cwd.join(pdf_path))
                .map_err(|e| format!("Percorso PDF non valido: {e}"))
        })?
    };

    let c_path = CString::new(abs_path.as_os_str().as_bytes())
        .map_err(|_| "Percorso PDF contiene byte nulli".to_string())?;

    let mut err: *mut GError = ptr::null_mut();
    let uri = unsafe { (api.g_filename_to_uri)(c_path.as_ptr(), ptr::null(), &mut err) };
    if uri.is_null() {
        return Err(format!(
            "Impossibile convertire percorso PDF in URI: {}",
            take_gerror(api, err)
        ));
    }

    let doc = unsafe { (api.poppler_document_new_from_file)(uri, ptr::null(), &mut err) };
    unsafe { (api.g_free)(uri as *mut c_void) };
    if doc.is_null() {
        return Err(format!(
            "Impossibile aprire PDF con Poppler: {}",
            take_gerror(api, err)
        ));
    }
    Ok(PopplerDocGuard(api, doc))
}

/// Reads `(total_pages, aspect_ratio, width_pts, height_pts)` directly via `libpoppler-glib`.
pub fn pdf_metadata_sync(pdf_path: &Path) -> Result<(usize, f32, f32, f32), String> {
    let api = PopplerApi::get()?;
    let doc = open_document(api, pdf_path)?;
    let n_pages = unsafe { (api.poppler_document_get_n_pages)(doc.1) };
    if n_pages <= 0 {
        return Ok((0, 1.4142, 595.28, 841.89));
    }

    let mut width_pts = 595.28_f32;
    let mut height_pts = 841.89_f32;
    let mut aspect = 1.4142_f32;

    let page_ptr = unsafe { (api.poppler_document_get_page)(doc.1, 0) };
    if !page_ptr.is_null() {
        let page = PopplerPageGuard(api, page_ptr);
        let mut pw: c_double = 0.0;
        let mut ph: c_double = 0.0;
        unsafe { (api.poppler_page_get_size)(page.1, &mut pw, &mut ph) };
        if pw > 1.0 && ph > 1.0 {
            width_pts = pw as f32;
            height_pts = ph as f32;
            aspect = height_pts / width_pts;
        }
    }

    Ok((n_pages as usize, aspect, width_pts, height_pts))
}

/// Renders the requested 1-based PDF pages directly to RGBA buffers using a single Poppler
/// document handle and a reusable Cairo image surface (zero subprocesses, zero PNG encode/decode).
pub fn render_pages_sync(
    pdf_path: &Path,
    pages: &[usize],
    target_width_px: u32,
    target_height_px: u32,
) -> Result<Vec<RenderedPage>, String> {
    if pages.is_empty() {
        return Ok(Vec::new());
    }

    let api = PopplerApi::get()?;
    let doc = open_document(api, pdf_path)?;
    let n_pages = unsafe { (api.poppler_document_get_n_pages)(doc.1) }.max(0) as usize;
    let w = target_width_px.clamp(240, 2800);
    let h = target_height_px.clamp(240, 4000);

    let surf_ptr =
        unsafe { (api.cairo_image_surface_create)(CAIRO_FORMAT_ARGB32, w as c_int, h as c_int) };
    if surf_ptr.is_null() {
        return Err("Impossibile allocare superficie Cairo".to_string());
    }
    let surface = CairoSurfaceGuard(api, surf_ptr);

    let cr_ptr = unsafe { (api.cairo_create)(surface.1) };
    if cr_ptr.is_null() {
        return Err("Impossibile creare contesto Cairo".to_string());
    }
    let cr = CairoCtxGuard(api, cr_ptr);

    let stride = unsafe { (api.cairo_image_surface_get_stride)(surface.1) };
    if stride < (w as c_int) * 4 {
        return Err("Stride superficie Cairo non valido".to_string());
    }
    let stride_usize = stride as usize;
    let w_usize = w as usize;
    let h_usize = h as usize;

    let mut out = Vec::with_capacity(pages.len());

    for &page_num in pages {
        if page_num == 0 || page_num > n_pages {
            continue;
        }
        let page_ptr = unsafe { (api.poppler_document_get_page)(doc.1, (page_num - 1) as c_int) };
        if page_ptr.is_null() {
            return Err(format!("Impossibile caricare pagina PDF {page_num}"));
        }
        let page = PopplerPageGuard(api, page_ptr);

        let mut pw: c_double = 0.0;
        let mut ph: c_double = 0.0;
        unsafe { (api.poppler_page_get_size)(page.1, &mut pw, &mut ph) };
        if pw <= 0.0 || ph <= 0.0 {
            return Err(format!("Dimensioni pagina PDF {page_num} non valide"));
        }

        unsafe {
            (api.cairo_save)(cr.1);
            (api.cairo_set_source_rgb)(cr.1, 1.0, 1.0, 1.0);
            (api.cairo_paint)(cr.1);
            (api.cairo_scale)(cr.1, (w as c_double) / pw, (h as c_double) / ph);
            (api.poppler_page_render)(page.1, cr.1);
            (api.cairo_restore)(cr.1);
            (api.cairo_surface_flush)(surface.1);
        }

        let data_ptr = unsafe { (api.cairo_image_surface_get_data)(surface.1) };
        if data_ptr.is_null() {
            return Err("Buffer superficie Cairo non leggibile".to_string());
        }

        let src_buf = unsafe { std::slice::from_raw_parts(data_ptr, stride_usize * h_usize) };
        let mut rgba = vec![255u8; w_usize * h_usize * 4];

        for y in 0..h_usize {
            let src_row = &src_buf[y * stride_usize..y * stride_usize + w_usize * 4];
            let dst_row = &mut rgba[y * w_usize * 4..(y + 1) * w_usize * 4];
            for (src_px, dst_px) in src_row.chunks_exact(4).zip(dst_row.chunks_exact_mut(4)) {
                let p = u32::from_ne_bytes([src_px[0], src_px[1], src_px[2], src_px[3]]);
                dst_px[0] = ((p >> 16) & 0xff) as u8;
                dst_px[1] = ((p >> 8) & 0xff) as u8;
                dst_px[2] = (p & 0xff) as u8;
                dst_px[3] = 255;
            }
        }

        out.push(RenderedPage {
            page_number: page_num,
            data: RenderedPageData::Rgba {
                width: w,
                height: h,
                bytes: rgba,
            },
        });
    }

    out.sort_by_key(|p| p.page_number);
    Ok(out)
}
