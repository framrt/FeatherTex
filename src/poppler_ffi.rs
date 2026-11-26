//! Fast in-process PDF metadata and Cairo RGBA rasterization via `libpoppler-glib` + `libcairo`.
//! Uses the exact same Poppler/Cairo/FreeType stack as `pdftocairo` and Linux browsers, without
//! spawning external processes or encoding/decoding PNGs (~25-30x faster than `pdftocairo -png`).

use crate::compiler::{RenderedPage, RenderedPageData};
use std::ffi::{c_char, c_double, c_int, c_void, CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

const CAIRO_FORMAT_ARGB32: c_int = 0;

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
    fn g_filename_to_uri(
        filename: *const c_char,
        hostname: *const c_char,
        error: *mut *mut GError,
    ) -> *mut c_char;
    fn g_free(mem: *mut c_void);
    fn g_error_free(error: *mut GError);
    fn g_object_unref(object: *mut c_void);

    fn poppler_document_new_from_file(
        uri: *const c_char,
        password: *const c_char,
        error: *mut *mut GError,
    ) -> *mut PopplerDocument;
    fn poppler_document_get_n_pages(document: *mut PopplerDocument) -> c_int;
    fn poppler_document_get_page(document: *mut PopplerDocument, index: c_int) -> *mut PopplerPage;
    fn poppler_page_get_size(
        page: *mut PopplerPage,
        width: *mut c_double,
        height: *mut c_double,
    );
    fn poppler_page_render(page: *mut PopplerPage, cairo: *mut CairoContext);

    fn cairo_image_surface_create(format: c_int, width: c_int, height: c_int) -> *mut CairoSurface;
    fn cairo_image_surface_get_data(surface: *mut CairoSurface) -> *mut u8;
    fn cairo_image_surface_get_stride(surface: *mut CairoSurface) -> c_int;
    fn cairo_surface_flush(surface: *mut CairoSurface);
    fn cairo_surface_destroy(surface: *mut CairoSurface);

    fn cairo_create(target: *mut CairoSurface) -> *mut CairoContext;
    fn cairo_destroy(cr: *mut CairoContext);
    fn cairo_save(cr: *mut CairoContext);
    fn cairo_restore(cr: *mut CairoContext);
    fn cairo_set_source_rgb(cr: *mut CairoContext, red: c_double, green: c_double, blue: c_double);
    fn cairo_paint(cr: *mut CairoContext);
    fn cairo_scale(cr: *mut CairoContext, sx: c_double, sy: c_double);
}

struct PopplerDocGuard(*mut PopplerDocument);
impl Drop for PopplerDocGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { g_object_unref(self.0 as *mut c_void) };
        }
    }
}

struct PopplerPageGuard(*mut PopplerPage);
impl Drop for PopplerPageGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { g_object_unref(self.0 as *mut c_void) };
        }
    }
}

struct CairoSurfaceGuard(*mut CairoSurface);
impl Drop for CairoSurfaceGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { cairo_surface_destroy(self.0) };
        }
    }
}

struct CairoCtxGuard(*mut CairoContext);
impl Drop for CairoCtxGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { cairo_destroy(self.0) };
        }
    }
}

fn take_gerror(err: *mut GError) -> String {
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
    unsafe { g_error_free(err) };
    msg
}

fn open_document(pdf_path: &Path) -> Result<PopplerDocGuard, String> {
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
    let uri = unsafe { g_filename_to_uri(c_path.as_ptr(), ptr::null(), &mut err) };
    if uri.is_null() {
        return Err(format!("Impossibile convertire percorso PDF in URI: {}", take_gerror(err)));
    }

    let doc = unsafe { poppler_document_new_from_file(uri, ptr::null(), &mut err) };
    unsafe { g_free(uri as *mut c_void) };
    if doc.is_null() {
        return Err(format!("Impossibile aprire PDF con Poppler: {}", take_gerror(err)));
    }
    Ok(PopplerDocGuard(doc))
}

/// Reads `(total_pages, aspect_ratio, width_pts, height_pts)` directly via `libpoppler-glib`.
pub fn pdf_metadata_sync(pdf_path: &Path) -> Result<(usize, f32, f32, f32), String> {
    let doc = open_document(pdf_path)?;
    let n_pages = unsafe { poppler_document_get_n_pages(doc.0) };
    if n_pages <= 0 {
        return Ok((0, 1.4142, 595.28, 841.89));
    }

    let mut width_pts = 595.28_f32;
    let mut height_pts = 841.89_f32;
    let mut aspect = 1.4142_f32;

    let page_ptr = unsafe { poppler_document_get_page(doc.0, 0) };
    if !page_ptr.is_null() {
        let page = PopplerPageGuard(page_ptr);
        let mut pw: c_double = 0.0;
        let mut ph: c_double = 0.0;
        unsafe { poppler_page_get_size(page.0, &mut pw, &mut ph) };
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

    let doc = open_document(pdf_path)?;
    let n_pages = unsafe { poppler_document_get_n_pages(doc.0) }.max(0) as usize;
    let w = target_width_px.clamp(240, 2800);
    let h = target_height_px.clamp(240, 4000);

    let surf_ptr = unsafe { cairo_image_surface_create(CAIRO_FORMAT_ARGB32, w as c_int, h as c_int) };
    if surf_ptr.is_null() {
        return Err("Impossibile allocare superficie Cairo".to_string());
    }
    let surface = CairoSurfaceGuard(surf_ptr);

    let cr_ptr = unsafe { cairo_create(surface.0) };
    if cr_ptr.is_null() {
        return Err("Impossibile creare contesto Cairo".to_string());
    }
    let cr = CairoCtxGuard(cr_ptr);

    let stride = unsafe { cairo_image_surface_get_stride(surface.0) };
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
        let page_ptr = unsafe { poppler_document_get_page(doc.0, (page_num - 1) as c_int) };
        if page_ptr.is_null() {
            return Err(format!("Impossibile caricare pagina PDF {page_num}"));
        }
        let page = PopplerPageGuard(page_ptr);

        let mut pw: c_double = 0.0;
        let mut ph: c_double = 0.0;
        unsafe { poppler_page_get_size(page.0, &mut pw, &mut ph) };
        if pw <= 0.0 || ph <= 0.0 {
            return Err(format!("Dimensioni pagina PDF {page_num} non valide"));
        }

        unsafe {
            cairo_save(cr.0);
            cairo_set_source_rgb(cr.0, 1.0, 1.0, 1.0);
            cairo_paint(cr.0);
            cairo_scale(cr.0, (w as c_double) / pw, (h as c_double) / ph);
            poppler_page_render(page.0, cr.0);
            cairo_restore(cr.0);
            cairo_surface_flush(surface.0);
        }

        let data_ptr = unsafe { cairo_image_surface_get_data(surface.0) };
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

