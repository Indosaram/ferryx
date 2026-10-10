use std::ffi::c_void;
use std::marker::PhantomData;
use std::ptr::NonNull;

use super::error::NativeTerminalError;
use super::sys::constants::{
    GHOSTTY_NO_VALUE, GHOSTTY_SNAPSHOT_DECODER_DATA_HISTORY_ROWS_ALTERNATE,
    GHOSTTY_SNAPSHOT_DECODER_DATA_HISTORY_ROWS_PRIMARY,
    GHOSTTY_SNAPSHOT_DECODER_DATA_MAX_CONTINUATION_BYTES,
    GHOSTTY_SNAPSHOT_DECODER_DATA_PROGRESS_REMAINING,
    GHOSTTY_SNAPSHOT_DECODER_DATA_PROGRESS_ROWS,
    GHOSTTY_SNAPSHOT_DECODER_DATA_PROGRESS_SCREEN,
    GHOSTTY_SNAPSHOT_DECODER_DATA_RETAIN_CONTINUATION,
    GHOSTTY_SNAPSHOT_DECODER_DATA_SOURCE_OFFSET,
    GHOSTTY_SNAPSHOT_DECODER_OPT_MAX_CONTINUATION_BYTES,
    GHOSTTY_SNAPSHOT_DECODER_OPT_RETAIN_CONTINUATION, GHOSTTY_SNAPSHOT_MAGIC,
    GHOSTTY_SNAPSHOT_VERSION_1, GHOSTTY_SUCCESS,
    GHOSTTY_TERMINAL_DATA_CONTINUATION_MAX_BYTES,
    GHOSTTY_TERMINAL_OPT_CONTINUATION_MAX_BYTES,
};
use super::sys::ffi::{
    ghostty_snapshot_decoder_decode, ghostty_snapshot_decoder_free,
    ghostty_snapshot_decoder_get, ghostty_snapshot_decoder_new_buf,
    ghostty_snapshot_decoder_next, ghostty_snapshot_decoder_ready,
    ghostty_snapshot_decoder_set, ghostty_snapshot_encode,
    ghostty_snapshot_encode_buf, ghostty_terminal_free, ghostty_terminal_get,
    ghostty_terminal_new, ghostty_terminal_set, ghostty_terminal_vt_write,
};
use super::sys::types::{
    GhosttySnapshotDecoder, GhosttySnapshotDecoderImpl, GhosttyTerminal,
    GhosttyTerminalImpl, GhosttyWriter,
};

pub const SNAPSHOT_MAGIC: &[u8; 8] = GHOSTTY_SNAPSHOT_MAGIC;
pub const SNAPSHOT_VERSION_CURRENT: u16 = GHOSTTY_SNAPSHOT_VERSION_1;
pub const MIN_SNAPSHOT_WIRE_BYTES: usize = 10;
pub const DEFAULT_MAX_SNAPSHOT_WIRE_BYTES: usize = 128 * 1024 * 1024;
pub const DEFAULT_MAX_CONTINUATION_BYTES: usize = 65 * 1024 * 1024;
pub const MAX_CONTINUATION_CEILING: usize = 65 * 1024 * 1024;

pub const DEFAULT_MAX_COLS: u16 = 1024;
pub const DEFAULT_MAX_ROWS: u16 = 1024;
pub const DEFAULT_MAX_SCREENS: u16 = 2;
pub const DEFAULT_MAX_SCROLLBACK_ROWS: u64 = 1_000_000;
pub const DEFAULT_MAX_SCROLLBACK_BYTES: u64 = 256 * 1024 * 1024;

pub const KITTY_VIRTUAL_PLACEHOLDER_CODEPOINT: u32 = 0x10EEEE;
pub const KITTY_GRAPHICS_IN_SNAPSHOT_SUPPORTED: bool = false;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotCodecOptions {
    pub max_continuation_bytes: usize,
    pub retain_continuation: bool,
    pub max_wire_bytes: usize,
    pub max_cols: u16,
    pub max_rows: u16,
    pub max_screens: u16,
    pub max_scrollback_rows: u64,
    pub max_scrollback_bytes: u64,
}

impl Default for SnapshotCodecOptions {
    fn default() -> Self {
        Self {
            max_continuation_bytes: DEFAULT_MAX_CONTINUATION_BYTES,
            retain_continuation: false,
            max_wire_bytes: DEFAULT_MAX_SNAPSHOT_WIRE_BYTES,
            max_cols: DEFAULT_MAX_COLS,
            max_rows: DEFAULT_MAX_ROWS,
            max_screens: DEFAULT_MAX_SCREENS,
            max_scrollback_rows: DEFAULT_MAX_SCROLLBACK_ROWS,
            max_scrollback_bytes: DEFAULT_MAX_SCROLLBACK_BYTES,
        }
    }
}

pub fn validate_snapshot_envelope(bytes: &[u8]) -> Result<u16, NativeTerminalError> {
    validate_snapshot_envelope_bounded(bytes, DEFAULT_MAX_SNAPSHOT_WIRE_BYTES)
}

pub fn validate_snapshot_envelope_bounded(
    bytes: &[u8],
    max_bytes: usize,
) -> Result<u16, NativeTerminalError> {
    if bytes.len() < MIN_SNAPSHOT_WIRE_BYTES {
        return Err(NativeTerminalError::InvalidValue(format!(
            "Snapshot length {} bytes is smaller than envelope minimum {} bytes",
            bytes.len(),
            MIN_SNAPSHOT_WIRE_BYTES
        )));
    }
    if bytes.len() > max_bytes {
        return Err(NativeTerminalError::LimitExceeded);
    }
    if &bytes[0..8] != SNAPSHOT_MAGIC {
        return Err(NativeTerminalError::InvalidValue(
            "Invalid snapshot magic header: expected 'GHOSTSNP'".to_string(),
        ));
    }
    let version = u16::from_le_bytes([bytes[8], bytes[9]]);
    if version != SNAPSHOT_VERSION_CURRENT {
        return Err(NativeTerminalError::InvalidValue(format!(
            "Unsupported snapshot version {version}: expected version {SNAPSHOT_VERSION_CURRENT}"
        )));
    }
    Ok(version)
}

pub fn validate_snapshot_bounds(
    bytes: &[u8],
    options: &SnapshotCodecOptions,
) -> Result<(), NativeTerminalError> {
    validate_snapshot_envelope_bounded(bytes, options.max_wire_bytes)?;

    if bytes.len() >= 20 {
        let tag = u16::from_le_bytes([bytes[10], bytes[11]]);
        if tag != 1 {
            return Err(NativeTerminalError::InvalidValue(format!(
                "First snapshot record must be TERMINAL (tag 1), found tag {tag}"
            )));
        }
        let payload_len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        if (payload_len as usize) > options.max_wire_bytes {
            return Err(NativeTerminalError::LimitExceeded);
        }
    }

    if bytes.len() >= 24 {
        let cols = u16::from_le_bytes([bytes[20], bytes[21]]);
        let rows = u16::from_le_bytes([bytes[22], bytes[23]]);
        if cols == 0 || rows == 0 {
            return Err(NativeTerminalError::InvalidDimensions(cols, rows));
        }
        if cols > options.max_cols || rows > options.max_rows {
            return Err(NativeTerminalError::LimitExceeded);
        }
    }

    if bytes.len() >= 45 {
        let screen_count = u16::from_le_bytes([bytes[43], bytes[44]]);
        if screen_count == 0 || screen_count > options.max_screens {
            return Err(NativeTerminalError::LimitExceeded);
        }
    }

    if bytes.len() >= 123 {
        let max_sb_bytes = u64::from_le_bytes([
            bytes[107], bytes[108], bytes[109], bytes[110],
            bytes[111], bytes[112], bytes[113], bytes[114],
        ]);
        let max_sb_rows = u64::from_le_bytes([
            bytes[115], bytes[116], bytes[117], bytes[118],
            bytes[119], bytes[120], bytes[121], bytes[122],
        ]);
        if max_sb_bytes > options.max_scrollback_bytes || max_sb_rows > options.max_scrollback_rows {
            return Err(NativeTerminalError::LimitExceeded);
        }
    }

    Ok(())
}

pub unsafe fn enable_raw_continuation_tracking(
    terminal: GhosttyTerminal,
    max_bytes: usize,
) -> Result<(), NativeTerminalError> {
    if terminal.is_null() {
        return Err(NativeTerminalError::InvalidValue(
            "Null terminal passed to enable_raw_continuation_tracking".to_string(),
        ));
    }
    let ceiling = max_bytes.min(MAX_CONTINUATION_CEILING);
    let res = ghostty_terminal_set(
        terminal,
        GHOSTTY_TERMINAL_OPT_CONTINUATION_MAX_BYTES,
        (&ceiling as *const usize).cast(),
    );
    NativeTerminalError::from_c_result(res, "ghostty_terminal_set(OPT_CONTINUATION_MAX_BYTES)")
}

pub unsafe fn query_raw_continuation_max_bytes(
    terminal: GhosttyTerminal,
) -> Result<usize, NativeTerminalError> {
    if terminal.is_null() {
        return Err(NativeTerminalError::InvalidValue(
            "Null terminal passed to query_raw_continuation_max_bytes".to_string(),
        ));
    }
    let mut out: usize = 0;
    let res = ghostty_terminal_get(
        terminal,
        GHOSTTY_TERMINAL_DATA_CONTINUATION_MAX_BYTES,
        (&mut out as *mut usize).cast(),
    );
    NativeTerminalError::from_c_result(res, "ghostty_terminal_get(DATA_CONTINUATION_MAX_BYTES)")?;
    Ok(out)
}

struct BoundedWriterContext<'a> {
    buffer: &'a mut Vec<u8>,
    max_bytes: usize,
    overflow: bool,
}

unsafe extern "C" fn bounded_writer_callback(
    userdata: *mut c_void,
    data: *const u8,
    len: usize,
) -> bool {
    if userdata.is_null() || data.is_null() {
        return false;
    }
    let ctx = &mut *(userdata as *mut BoundedWriterContext);
    if ctx.overflow {
        return false;
    }
    if ctx.buffer.len().saturating_add(len) > ctx.max_bytes {
        ctx.overflow = true;
        return false;
    }
    let slice = std::slice::from_raw_parts(data, len);
    ctx.buffer.extend_from_slice(slice);
    true
}

pub unsafe fn encode_raw_terminal_snapshot(
    terminal: GhosttyTerminal,
    max_wire_bytes: usize,
) -> Result<Vec<u8>, NativeTerminalError> {
    if terminal.is_null() {
        return Err(NativeTerminalError::InvalidValue(
            "Null terminal passed to encode_raw_terminal_snapshot".to_string(),
        ));
    }
    let mut out = Vec::new();
    let mut ctx = BoundedWriterContext {
        buffer: &mut out,
        max_bytes: max_wire_bytes,
        overflow: false,
    };
    let writer = GhosttyWriter {
        write: bounded_writer_callback,
        userdata: (&mut ctx as *mut BoundedWriterContext).cast(),
    };

    let res = ghostty_snapshot_encode(terminal, writer);
    if ctx.overflow {
        return Err(NativeTerminalError::LimitExceeded);
    }
    NativeTerminalError::from_c_result(res, "ghostty_snapshot_encode")?;
    validate_snapshot_envelope_bounded(&out, max_wire_bytes)?;
    Ok(out)
}

pub unsafe fn encode_raw_terminal_snapshot_buf(
    terminal: GhosttyTerminal,
    buf: &mut [u8],
) -> Result<usize, NativeTerminalError> {
    if terminal.is_null() {
        return Err(NativeTerminalError::InvalidValue(
            "Null terminal passed to encode_raw_terminal_snapshot_buf".to_string(),
        ));
    }
    let mut written: usize = 0;
    let res = ghostty_snapshot_encode_buf(
        terminal,
        buf.as_mut_ptr(),
        buf.len(),
        &mut written as *mut usize,
    );
    NativeTerminalError::from_c_result(res, "ghostty_snapshot_encode_buf")?;
    Ok(written)
}

pub unsafe fn encode_raw_terminal_snapshot_to_writer<W: std::io::Write>(
    terminal: GhosttyTerminal,
    mut writer: W,
    max_wire_bytes: usize,
) -> Result<usize, NativeTerminalError> {
    if terminal.is_null() {
        return Err(NativeTerminalError::InvalidValue(
            "Null terminal passed to encode_raw_terminal_snapshot_to_writer".to_string(),
        ));
    }

    struct StreamContext<W> {
        writer: W,
        total_written: usize,
        max_bytes: usize,
        overflow: bool,
        io_error: Option<std::io::Error>,
    }

    unsafe extern "C" fn stream_callback<W: std::io::Write>(
        userdata: *mut c_void,
        data: *const u8,
        len: usize,
    ) -> bool {
        if userdata.is_null() || data.is_null() {
            return false;
        }
        let ctx = &mut *(userdata as *mut StreamContext<W>);
        if ctx.overflow || ctx.io_error.is_some() {
            return false;
        }
        if ctx.total_written.saturating_add(len) > ctx.max_bytes {
            ctx.overflow = true;
            return false;
        }
        let slice = std::slice::from_raw_parts(data, len);
        match ctx.writer.write_all(slice) {
            Ok(()) => {
                ctx.total_written += len;
                true
            }
            Err(e) => {
                ctx.io_error = Some(e);
                false
            }
        }
    }

    let mut ctx = StreamContext {
        writer,
        total_written: 0,
        max_bytes: max_wire_bytes,
        overflow: false,
        io_error: None,
    };
    let c_writer = GhosttyWriter {
        write: stream_callback::<W>,
        userdata: (&mut ctx as *mut StreamContext<W>).cast(),
    };

    let res = ghostty_snapshot_encode(terminal, c_writer);
    if ctx.overflow {
        return Err(NativeTerminalError::LimitExceeded);
    }
    if let Some(err) = ctx.io_error {
        return Err(NativeTerminalError::IoError(err.to_string()));
    }
    NativeTerminalError::from_c_result(res, "ghostty_snapshot_encode(stream)")?;
    Ok(ctx.total_written)
}

pub fn encode_terminal_snapshot(
    terminal: &DecodedTerminal,
    max_wire_bytes: usize,
) -> Result<Vec<u8>, NativeTerminalError> {
    unsafe { encode_raw_terminal_snapshot(terminal.as_raw(), max_wire_bytes) }
}

pub fn encode_terminal_snapshot_buf(
    terminal: &DecodedTerminal,
    buf: &mut [u8],
) -> Result<usize, NativeTerminalError> {
    unsafe { encode_raw_terminal_snapshot_buf(terminal.as_raw(), buf) }
}

pub fn encode_terminal_snapshot_to_writer<W: std::io::Write>(
    terminal: &DecodedTerminal,
    writer: W,
    max_wire_bytes: usize,
) -> Result<usize, NativeTerminalError> {
    unsafe { encode_raw_terminal_snapshot_to_writer(terminal.as_raw(), writer, max_wire_bytes) }
}

pub struct DecodedTerminal {
    handle: NonNull<GhosttyTerminalImpl>,
}

unsafe impl Send for DecodedTerminal {}

impl DecodedTerminal {
    pub fn new(cols: u16, rows: u16) -> Result<Self, NativeTerminalError> {
        if cols == 0 || rows == 0 {
            return Err(NativeTerminalError::InvalidDimensions(cols, rows));
        }
        let mut raw: GhosttyTerminal = std::ptr::null_mut();
        let res = unsafe { ghostty_terminal_new(std::ptr::null(), &mut raw, cols, rows) };
        NativeTerminalError::from_c_result(res, "ghostty_terminal_new")?;
        unsafe { Self::from_raw(raw) }
    }

    pub unsafe fn from_raw(raw: GhosttyTerminal) -> Result<Self, NativeTerminalError> {
        let handle = NonNull::new(raw).ok_or_else(|| {
            NativeTerminalError::InvalidValue(
                "Null terminal pointer passed to DecodedTerminal::from_raw".to_string(),
            )
        })?;
        Ok(Self { handle })
    }

    pub fn as_raw(&self) -> GhosttyTerminal {
        self.handle.as_ptr()
    }

    pub fn non_null(&self) -> NonNull<GhosttyTerminalImpl> {
        self.handle
    }

    pub fn into_raw(self) -> GhosttyTerminal {
        let ptr = self.handle.as_ptr();
        std::mem::forget(self);
        ptr
    }

    pub fn enable_continuation_tracking(
        &mut self,
        max_bytes: usize,
    ) -> Result<(), NativeTerminalError> {
        unsafe { enable_raw_continuation_tracking(self.handle.as_ptr(), max_bytes) }
    }

    pub fn continuation_max_bytes(&self) -> Result<usize, NativeTerminalError> {
        unsafe { query_raw_continuation_max_bytes(self.handle.as_ptr()) }
    }

    pub fn encode(&self, max_wire_bytes: usize) -> Result<Vec<u8>, NativeTerminalError> {
        encode_terminal_snapshot(self, max_wire_bytes)
    }

    pub fn encode_buf(&self, buf: &mut [u8]) -> Result<usize, NativeTerminalError> {
        encode_terminal_snapshot_buf(self, buf)
    }

    pub fn cols(&self) -> Result<u16, NativeTerminalError> {
        super::queries::query_cols(self.handle)
    }

    pub fn rows(&self) -> Result<u16, NativeTerminalError> {
        super::queries::query_rows(self.handle)
    }

    pub fn is_alternate_screen(&self) -> Result<bool, NativeTerminalError> {
        super::queries::query_is_alternate_screen(self.handle)
    }

    pub fn cursor_position(&self) -> Result<(u16, u16), NativeTerminalError> {
        super::queries::query_cursor_position(self.handle)
    }

    pub fn vt_write(&mut self, data: &[u8]) {
        unsafe {
            ghostty_terminal_vt_write(self.handle.as_ptr(), data.as_ptr(), data.len());
        }
    }
}

impl Drop for DecodedTerminal {
    fn drop(&mut self) {
        unsafe {
            ghostty_terminal_free(self.handle.as_ptr());
        }
    }
}

pub struct IncrementalSnapshotDecoder<'a> {
    decoder: Option<NonNull<GhosttySnapshotDecoderImpl>>,
    terminal: Option<DecodedTerminal>,
    _marker: PhantomData<&'a [u8]>,
}

impl<'a> IncrementalSnapshotDecoder<'a> {
    pub fn start(
        bytes: &'a [u8],
        options: SnapshotCodecOptions,
    ) -> Result<Self, NativeTerminalError> {
        validate_snapshot_bounds(bytes, &options)?;

        if options.max_continuation_bytes > MAX_CONTINUATION_CEILING {
            return Err(NativeTerminalError::LimitExceeded);
        }

        let mut raw_decoder: GhosttySnapshotDecoder = std::ptr::null_mut();
        let res = unsafe {
            ghostty_snapshot_decoder_new_buf(
                std::ptr::null(),
                &mut raw_decoder,
                bytes.as_ptr(),
                bytes.len(),
            )
        };
        NativeTerminalError::from_c_result(res, "ghostty_snapshot_decoder_new_buf")?;

        let decoder_handle = NonNull::new(raw_decoder).ok_or_else(|| {
            NativeTerminalError::InvalidValue(
                "ghostty_snapshot_decoder_new_buf returned null decoder pointer".to_string(),
            )
        })?;

        let set_res = unsafe {
            ghostty_snapshot_decoder_set(
                decoder_handle.as_ptr(),
                GHOSTTY_SNAPSHOT_DECODER_OPT_MAX_CONTINUATION_BYTES,
                (&options.max_continuation_bytes as *const usize).cast(),
            )
        };
        if let Err(e) = NativeTerminalError::from_c_result(set_res, "set(OPT_MAX_CONTINUATION_BYTES)") {
            unsafe { ghostty_snapshot_decoder_free(decoder_handle.as_ptr()) };
            return Err(e);
        }

        let set_retain_res = unsafe {
            ghostty_snapshot_decoder_set(
                decoder_handle.as_ptr(),
                GHOSTTY_SNAPSHOT_DECODER_OPT_RETAIN_CONTINUATION,
                (&options.retain_continuation as *const bool).cast(),
            )
        };
        if let Err(e) = NativeTerminalError::from_c_result(set_retain_res, "set(OPT_RETAIN_CONTINUATION)") {
            unsafe { ghostty_snapshot_decoder_free(decoder_handle.as_ptr()) };
            return Err(e);
        }

        let mut raw_term: GhosttyTerminal = std::ptr::null_mut();
        let ready_res = unsafe {
            ghostty_snapshot_decoder_ready(decoder_handle.as_ptr(), &mut raw_term)
        };
        if let Err(e) = NativeTerminalError::from_c_result(ready_res, "ghostty_snapshot_decoder_ready") {
            unsafe { ghostty_snapshot_decoder_free(decoder_handle.as_ptr()) };
            return Err(e);
        }

        let terminal = match unsafe { DecodedTerminal::from_raw(raw_term) } {
            Ok(t) => t,
            Err(e) => {
                unsafe { ghostty_snapshot_decoder_free(decoder_handle.as_ptr()) };
                return Err(e);
            }
        };

        Ok(Self {
            decoder: Some(decoder_handle),
            terminal: Some(terminal),
            _marker: PhantomData,
        })
    }

    pub fn terminal(&self) -> &DecodedTerminal {
        self.terminal.as_ref().expect("terminal present during decode")
    }

    pub fn terminal_mut(&mut self) -> &mut DecodedTerminal {
        self.terminal.as_mut().expect("terminal present during decode")
    }

    pub fn next_page(&mut self) -> Result<bool, NativeTerminalError> {
        let dec_ptr = self.decoder.expect("decoder present").as_ptr();
        let res = unsafe { ghostty_snapshot_decoder_next(dec_ptr) };
        match res {
            GHOSTTY_SUCCESS => Ok(true),
            GHOSTTY_NO_VALUE => Ok(false),
            other => {
                NativeTerminalError::from_c_result(other, "ghostty_snapshot_decoder_next")?;
                Ok(false)
            }
        }
    }

    pub fn source_offset(&self) -> Result<usize, NativeTerminalError> {
        let dec_ptr = self.decoder.expect("decoder present").as_ptr();
        let mut offset: usize = 0;
        let res = unsafe {
            ghostty_snapshot_decoder_get(
                dec_ptr,
                GHOSTTY_SNAPSHOT_DECODER_DATA_SOURCE_OFFSET,
                (&mut offset as *mut usize).cast(),
            )
        };
        NativeTerminalError::from_c_result(res, "ghostty_snapshot_decoder_get(DATA_SOURCE_OFFSET)")?;
        Ok(offset)
    }

    pub fn max_continuation_bytes(&self) -> Result<usize, NativeTerminalError> {
        let dec_ptr = self.decoder.expect("decoder present").as_ptr();
        let mut out: usize = 0;
        let res = unsafe {
            ghostty_snapshot_decoder_get(
                dec_ptr,
                GHOSTTY_SNAPSHOT_DECODER_DATA_MAX_CONTINUATION_BYTES,
                (&mut out as *mut usize).cast(),
            )
        };
        NativeTerminalError::from_c_result(res, "ghostty_snapshot_decoder_get(DATA_MAX_CONTINUATION_BYTES)")?;
        Ok(out)
    }

    pub fn retain_continuation(&self) -> Result<bool, NativeTerminalError> {
        let dec_ptr = self.decoder.expect("decoder present").as_ptr();
        let mut out: bool = false;
        let res = unsafe {
            ghostty_snapshot_decoder_get(
                dec_ptr,
                GHOSTTY_SNAPSHOT_DECODER_DATA_RETAIN_CONTINUATION,
                (&mut out as *mut bool).cast(),
            )
        };
        NativeTerminalError::from_c_result(res, "ghostty_snapshot_decoder_get(DATA_RETAIN_CONTINUATION)")?;
        Ok(out)
    }

    pub fn history_rows_primary(&self) -> Result<u64, NativeTerminalError> {
        let dec_ptr = self.decoder.expect("decoder present").as_ptr();
        let mut rows: u64 = 0;
        let res = unsafe {
            ghostty_snapshot_decoder_get(
                dec_ptr,
                GHOSTTY_SNAPSHOT_DECODER_DATA_HISTORY_ROWS_PRIMARY,
                (&mut rows as *mut u64).cast(),
            )
        };
        NativeTerminalError::from_c_result(res, "ghostty_snapshot_decoder_get(DATA_HISTORY_ROWS_PRIMARY)")?;
        Ok(rows)
    }

    pub fn history_rows_alternate(&self) -> Result<u64, NativeTerminalError> {
        let dec_ptr = self.decoder.expect("decoder present").as_ptr();
        let mut rows: u64 = 0;
        let res = unsafe {
            ghostty_snapshot_decoder_get(
                dec_ptr,
                GHOSTTY_SNAPSHOT_DECODER_DATA_HISTORY_ROWS_ALTERNATE,
                (&mut rows as *mut u64).cast(),
            )
        };
        NativeTerminalError::from_c_result(res, "ghostty_snapshot_decoder_get(DATA_HISTORY_ROWS_ALTERNATE)")?;
        Ok(rows)
    }

    pub fn progress_rows(&self) -> Result<usize, NativeTerminalError> {
        let dec_ptr = self.decoder.expect("decoder present").as_ptr();
        let mut rows: usize = 0;
        let res = unsafe {
            ghostty_snapshot_decoder_get(
                dec_ptr,
                GHOSTTY_SNAPSHOT_DECODER_DATA_PROGRESS_ROWS,
                (&mut rows as *mut usize).cast(),
            )
        };
        NativeTerminalError::from_c_result(res, "ghostty_snapshot_decoder_get(DATA_PROGRESS_ROWS)")?;
        Ok(rows)
    }

    pub fn progress_remaining(&self) -> Result<u32, NativeTerminalError> {
        let dec_ptr = self.decoder.expect("decoder present").as_ptr();
        let mut count: u32 = 0;
        let res = unsafe {
            ghostty_snapshot_decoder_get(
                dec_ptr,
                GHOSTTY_SNAPSHOT_DECODER_DATA_PROGRESS_REMAINING,
                (&mut count as *mut u32).cast(),
            )
        };
        NativeTerminalError::from_c_result(res, "ghostty_snapshot_decoder_get(DATA_PROGRESS_REMAINING)")?;
        Ok(count)
    }

    pub fn progress_screen(&self) -> Result<i32, NativeTerminalError> {
        let dec_ptr = self.decoder.expect("decoder present").as_ptr();
        let mut screen: std::ffi::c_int = 0;
        let res = unsafe {
            ghostty_snapshot_decoder_get(
                dec_ptr,
                GHOSTTY_SNAPSHOT_DECODER_DATA_PROGRESS_SCREEN,
                (&mut screen as *mut std::ffi::c_int).cast(),
            )
        };
        NativeTerminalError::from_c_result(res, "ghostty_snapshot_decoder_get(DATA_PROGRESS_SCREEN)")?;
        Ok(screen)
    }

    pub fn finish(mut self) -> DecodedTerminal {
        if let Some(dec) = self.decoder.take() {
            unsafe {
                ghostty_snapshot_decoder_free(dec.as_ptr());
            }
        }
        self.terminal.take().expect("terminal present on finish")
    }
}

impl<'a> Drop for IncrementalSnapshotDecoder<'a> {
    fn drop(&mut self) {
        if let Some(dec) = self.decoder.take() {
            unsafe {
                ghostty_snapshot_decoder_free(dec.as_ptr());
            }
        }
    }
}

pub fn decode_terminal_snapshot(
    bytes: &[u8],
    options: SnapshotCodecOptions,
) -> Result<DecodedTerminal, NativeTerminalError> {
    validate_snapshot_bounds(bytes, &options)?;

    if options.max_continuation_bytes > MAX_CONTINUATION_CEILING {
        return Err(NativeTerminalError::LimitExceeded);
    }

    let mut raw_decoder: GhosttySnapshotDecoder = std::ptr::null_mut();
    let res = unsafe {
        ghostty_snapshot_decoder_new_buf(
            std::ptr::null(),
            &mut raw_decoder,
            bytes.as_ptr(),
            bytes.len(),
        )
    };
    NativeTerminalError::from_c_result(res, "ghostty_snapshot_decoder_new_buf")?;

    let decoder_handle = NonNull::new(raw_decoder).ok_or_else(|| {
        NativeTerminalError::InvalidValue(
            "ghostty_snapshot_decoder_new_buf returned null decoder pointer".to_string(),
        )
    })?;

    struct DecoderGuard(NonNull<GhosttySnapshotDecoderImpl>);
    impl Drop for DecoderGuard {
        fn drop(&mut self) {
            unsafe { ghostty_snapshot_decoder_free(self.0.as_ptr()) };
        }
    }
    let _guard = DecoderGuard(decoder_handle);

    let set_opt_res = unsafe {
        ghostty_snapshot_decoder_set(
            decoder_handle.as_ptr(),
            GHOSTTY_SNAPSHOT_DECODER_OPT_MAX_CONTINUATION_BYTES,
            (&options.max_continuation_bytes as *const usize).cast(),
        )
    };
    NativeTerminalError::from_c_result(set_opt_res, "set(OPT_MAX_CONTINUATION_BYTES)")?;

    let set_retain_res = unsafe {
        ghostty_snapshot_decoder_set(
            decoder_handle.as_ptr(),
            GHOSTTY_SNAPSHOT_DECODER_OPT_RETAIN_CONTINUATION,
            (&options.retain_continuation as *const bool).cast(),
        )
    };
    NativeTerminalError::from_c_result(set_retain_res, "set(OPT_RETAIN_CONTINUATION)")?;

    let mut raw_term: GhosttyTerminal = std::ptr::null_mut();
    let decode_res = unsafe {
        ghostty_snapshot_decoder_decode(decoder_handle.as_ptr(), &mut raw_term)
    };
    NativeTerminalError::from_c_result(decode_res, "ghostty_snapshot_decoder_decode")?;

    unsafe { DecodedTerminal::from_raw(raw_term) }
}
