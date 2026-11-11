use core::ptr::{read_volatile, write_volatile};

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Address(u32);
impl Address {
    fn at(self, offset: u32) -> Self { Self(self.0.wrapping_add(offset)) }
    unsafe fn word(self) -> u32 { unsafe { read_volatile(self.0 as *const u32) } }
    unsafe fn half(self) -> u16 { unsafe { read_volatile(self.0 as *const u16) } }
    unsafe fn byte(self) -> u8 { unsafe { read_volatile(self.0 as *const u8) } }
    unsafe fn put(self, value: u32) { unsafe { write_volatile(self.0 as *mut u32, value) }; }
    unsafe fn put_half(self, value: u16) { unsafe { write_volatile(self.0 as *mut u16, value) }; }
    unsafe fn put_byte(self, value: u8) { unsafe { write_volatile(self.0 as *mut u8, value) }; }
}
unsafe fn copy_bytes(destination: Address, source: Address, length: u32) {
    for index in 0..length { unsafe { destination.at(index).put_byte(source.at(index).byte()) }; }
}
unsafe fn clear_bytes(destination: Address, length: u32) {
    for index in 0..length { unsafe { destination.at(index).put_byte(0) }; }
}
fn fatal() -> ! { loop { core::hint::spin_loop(); } }

#[derive(Clone, Copy)]
#[repr(transparent)]
struct WorkerId(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct RegistrySlot(i16);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct EventCode(u16);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct Completion(u32);
impl Completion {
    unsafe fn call(self, status: i32) {
        if self.0 != 0 {
            let callback: unsafe extern "C" fn(i32) = unsafe { core::mem::transmute(self.0) };
            unsafe { callback(status) };
        }
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct RegistryMessage {
    code: u16,
    slot: u8,
    queue: u8,
    first: u32,
    second: u32,
    third: i32,
}
#[repr(C)]
struct QueueMessage {
    code: u16,
    sender: u8,
    destination: u8,
    length: u32,
    payload: *const RegistryMessage,
}

unsafe extern "C" {
    fn xm1_pipeline_image_send_message(channel: u32, message: *const QueueMessage) -> i32;
    fn xm1_pipeline_image_receive_message(channel: u32, message: *mut QueueMessage, timeout: i32) -> i32;
    fn xm1_pipeline_image_current_task() -> u32;
    fn xm1_pipeline_image_initialize_quality_tables();
    fn xm1_pipeline_image_find_quality_profile(width: u32, height: u32) -> u32;
    fn xm1_pipeline_image_select_raw_buffer(phase: u32, image: u32);
    fn xm1_pipeline_image_advance_raw_output(phase: u32, image: u32);
    fn xm1_pipeline_image_relocate_jpeg(image: u32, relocate: i32);
    fn xm1_pipeline_image_align_jpeg(image: u32);
    fn xm1_pipeline_image_fill_jpeg_padding(destination: u32, size: u32, resume: i32);
    fn xm1_pipeline_image_finish_exif(total: u32, remainder: u32);
    fn xm1_pipeline_image_diagnostic_event(group: u32, code: u32);
    fn xm1_pipeline_image_configure_task(task: u32, priority: u32) -> i32;
    fn xm1_develop_append_stream_text(category: u32, text: u32);
    fn xm1_pipeline_image_warp_initialize() -> u32;
    fn xm1_pipeline_image_warp_configure(configuration: u32) -> u32;
    fn xm1_pipeline_image_warp_start();
    fn xm1_pipeline_image_warp_cancel();
    fn xm1_pipeline_image_warp_release();
}

fn registry(worker: WorkerId) -> Address { Address(0x0096e04cu32.wrapping_add(worker.0.wrapping_mul(12))) }
unsafe fn queue_id(worker: WorkerId) -> u8 { unsafe { Address(0x007d088c).at(worker.0).byte() } }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_registry_initialize(registry: u32, capacity: u32, entries: u32) {
    let registry = Address(registry);
    unsafe { registry.at(4).put(capacity); registry.at(8).put(entries) };
    for index in 0..capacity { unsafe { Address(entries).at(index.wrapping_mul(8)).put(u32::MAX) }; }
    unsafe { registry.put_half(0); registry.at(2).put_half(0) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_registry_allocate(registry: u32, key: i32) -> i32 {
    let registry = Address(registry);
    let capacity = unsafe { registry.at(4).word() };
    if capacity == 0 { return -1; }
    let entries = Address(unsafe { registry.at(8).word() });
    let mut cursor = unsafe { registry.half() };
    let mut scanned = 0;
    while unsafe { entries.at(u32::from(cursor).wrapping_mul(8)).word() } != u32::MAX {
        cursor = (u32::from(cursor.wrapping_add(1)) % capacity) as u16;
        unsafe { registry.put_half(cursor) };
        scanned += 1;
        if scanned >= capacity { return -1; }
    }
    unsafe { entries.at(u32::from(cursor).wrapping_mul(8)).put(key as u32) };
    let selected = unsafe { registry.half() };
    unsafe { registry.put_half((u32::from(selected.wrapping_add(1)) % capacity) as u16) };
    i32::from(selected as i16)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_registry_free(registry: u32, index: i32) {
    if index < 0 { return; }
    let registry = Address(registry);
    let entries = Address(unsafe { registry.at(8).word() });
    unsafe { entries.at((index as u32).wrapping_mul(8)).put(u32::MAX) };
    if unsafe { registry.at(2).half() } != index as u16 { return; }
    let capacity = unsafe { registry.at(4).word() };
    if capacity == 0 { return; }
    let mut scanned = 0u16;
    loop {
        let cursor = unsafe { registry.at(2).half() }.wrapping_add(1);
        let cursor = (u32::from(cursor) % capacity) as u16;
        unsafe { registry.at(2).put_half(cursor) };
        if unsafe { entries.at(u32::from(cursor).wrapping_mul(8)).word() } == 0 || cursor == unsafe { registry.half() } { return; }
        scanned = scanned.wrapping_add(1);
        if u32::from(scanned) >= capacity { return; }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_registry_get_key(registry: u32, index: i32) -> i32 {
    if index < 0 { 0 } else { unsafe { Address(Address(registry).at(8).word()).at((index as u32).wrapping_mul(8)).word() as i32 } }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_registry_get_request(registry: u32, index: i32) -> u32 {
    if index < 0 { 0 } else { unsafe { Address(Address(registry).at(8).word()).at((index as u32).wrapping_mul(8).wrapping_add(4)).word() } }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_registry_set_request(registry: u32, index: i32, request: u32) {
    if index >= 0 { unsafe { Address(Address(registry).at(8).word()).at((index as u32).wrapping_mul(8).wrapping_add(4)).put(request) }; }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_registry_lookup(registry: u32, index: i32) -> u32 {
    if unsafe { xm1_pipeline_image_registry_get_key(registry, index) } == -1 { 0 } else { unsafe { xm1_pipeline_image_registry_get_request(registry, index) } }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_request_register(request: u32) -> i32 {
    let request = Address(request);
    let registry = registry(WorkerId(unsafe { request.word() }));
    let slot = RegistrySlot(unsafe { xm1_pipeline_image_registry_allocate(registry.0, 0) } as i16);
    if slot.0 == -1 { unsafe { Address(0x008faccc).put_half(1) }; }
    unsafe { xm1_pipeline_image_registry_set_request(registry.0, i32::from(slot.0), request.0); request.at(4).put_half(slot.0 as u16) };
    i32::from(slot.0)
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_request_initialize(request: u32, worker: u32, completion_mode: i32, dispatch_kind: i32, table_index: i32, codes: u32, handlers: u32, context: u32) {
    let request = Address(request);
    unsafe { clear_bytes(request, 32); request.put(worker); request.at(8).put(dispatch_kind as u32); request.at(6).put_half(completion_mode as u16); request.at(16).put(codes); request.at(20).put(handlers); request.at(12).put(table_index as u32); request.at(28).put(context); xm1_pipeline_image_request_register(request.0) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_request_unregister(request: u32) {
    let request = Address(request);
    let worker = WorkerId(u32::from(unsafe { request.byte() }));
    unsafe { xm1_pipeline_image_registry_free(registry(worker).0, i32::from(request.at(4).half() as i16)) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_request_send(request: u32, code: u16, first: u32, second: u32, third: i32) {
    let request = Address(request);
    let worker = WorkerId(u32::from(unsafe { request.byte() }));
    let queue = unsafe { queue_id(worker) };
    let code = EventCode(code);
    let payload = RegistryMessage { code: code.0, slot: unsafe { request.at(4).half() } as u8, queue, first, second, third };
    let sender = unsafe { xm1_pipeline_image_current_task() } as u8;
    let header = QueueMessage { code: code.0, sender, destination: queue, length: 16, payload: &payload };
    unsafe { xm1_pipeline_image_send_message(u32::from(queue), &header) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_request_send_reply(message: *const RegistryMessage) {
    let message = unsafe { read_volatile(message) };
    let code = message.code | 0x8000;
    let payload = RegistryMessage { code, slot: message.queue, queue: message.slot, first: 0, second: 0, third: 0 };
    let header = QueueMessage { code, sender: message.queue, destination: message.slot, length: 16, payload: &payload };
    unsafe { xm1_pipeline_image_send_message(u32::from(message.slot), &header) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_request_decode(message: *const RegistryMessage, codes: u32) -> i32 {
    let code = unsafe { read_volatile(message) }.code;
    let codes = Address(codes);
    let mut index = 0u16;
    loop {
        let entry = codes.at(u32::from(index) * 8);
        let candidate = unsafe { entry.half() };
        if candidate == u16::MAX { return 0x5b; }
        if candidate == code { return unsafe { entry.at(4).word() } as i32; }
        index = index.wrapping_add(1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_request_dispatch(request: u32, code: i32, handlers: u32) -> i32 {
    let mut handlers = Address(handlers);
    loop {
        let candidate = unsafe { handlers.word() } as i32;
        if candidate == code || candidate == 0x5b { break; }
        handlers = handlers.at(8);
    }
    let callback = unsafe { handlers.at(4).word() };
    if callback == 0 { return 0; }
    let completion = unsafe { Address(request).at(6).half() } as i16;
    let callback: unsafe extern "C" fn(u32, i32, i32) = unsafe { core::mem::transmute(callback) };
    unsafe { callback(request, code, i32::from(completion)) };
    1
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_registry_process(registry: u32, message: *mut RegistryMessage) {
    let request = Address(unsafe { xm1_pipeline_image_registry_lookup(registry, i32::from(read_volatile(message).slot)) });
    if request.0 == 0 { return; }
    unsafe { request.at(24).put(message as u32) };
    let code = unsafe { xm1_pipeline_image_request_decode(message, request.at(16).word()) };
    let table = unsafe { request.at(12).word() };
    let handlers = unsafe { Address(request.at(20).word()).at(table.wrapping_mul(4)).word() };
    unsafe { xm1_pipeline_image_request_dispatch(request.0, code, handlers) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_registry_receive(worker: u32, message: *mut RegistryMessage) {
    let queue = unsafe { queue_id(WorkerId(worker)) };
    let mut packet = QueueMessage { code: 0, sender: 0, destination: queue, length: 16, payload: message };
    unsafe { xm1_pipeline_image_receive_message(u32::from(queue), &mut packet, -1) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_registry_worker() -> ! {
    let worker = WorkerId(unsafe { Address(0x008facc0).word() });
    let registry = registry(worker);
    unsafe { xm1_pipeline_image_registry_initialize(registry.0, 16, 0x0095761cu32.wrapping_add(worker.0.wrapping_mul(128))); Address(0x008facc0).put(worker.0.wrapping_add(1)) };
    loop {
        let mut message = RegistryMessage { code: 0, slot: 0, queue: 0, first: 0, second: 0, third: 0 };
        unsafe { xm1_pipeline_image_registry_receive(worker.0, &mut message); xm1_pipeline_image_registry_process(registry.0, &mut message) };
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Image(Address);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct MetadataRequest(Address);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct ImageInput(Address);
#[derive(Clone, Copy)]
enum CompressionKind { Thumbnail, Primary, Secondary }
impl CompressionKind {
    fn value(self) -> i32 { match self { Self::Thumbnail => 0, Self::Primary => 1, Self::Secondary => 2 } }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_initialize_request(request: u32, mode: u32, width: u32, height: u32, kind: i32, input: u32, target: u32, attempts: i32, option: u32, choice: u32, source: u32, destination: u32, limit: u32, stride: u32, controls: u32) {
    let request = MetadataRequest(Address(request));
    for (offset, value) in [(12, kind as u32), (16, input), (20, target), (24, option), (28, choice), (0xa0, width), (0xa4, height), (0xa8, source), (0xac, destination), (0xb0, limit), (0xbc, 0), (0, mode & 0xffff), (4, attempts as u32), (0x24, 0), (0x20, 0), (0xd0, 0), (0xc4, 1), (0xd8, xm1_pipeline_image_metadata_request_completed as *const () as u32), (0xd4, stride)] {
        unsafe { request.0.at(offset).put(value) };
    }
    if controls != 0 {
        unsafe { copy_bytes(request.0.at(0xdc), Address(controls), 80); request.0.at(0xc0).put(1); request.0.at(0x128).put(xm1_pipeline_image_metadata_request_halt as *const () as u32) };
    } else {
        unsafe { clear_bytes(request.0.at(0xdc), 80); request.0.at(0x128).put(0); request.0.at(0xc0).put(0) };
    }
}

unsafe fn initialize_input(image: Image, offset: u32, input: ImageInput, kind: CompressionKind, mode: u32, width: u32, height: u32, attempts: i32, option: u32, choice: u32) {
    unsafe { xm1_pipeline_image_metadata_initialize_request(image.0.at(offset).0, mode, width, height, kind.value(), input.0.at(28).word(), input.0.at(32).word(), attempts, option, choice, input.0.at(12).word(), input.0.at(16).word(), input.0.at(20).word(), input.0.at(40).word(), input.0.at(36).word()) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_setup_descriptors(image: u32, source: u32, thumbnail: u32, primary: u32, secondary: u32, scale: u32, order: u32, flags: u32, kind: u32, initialize: i32) {
    let image = Image(Address(image));
    if initialize != 0 {
        unsafe { image.0.at(24).put(0) };
        if unsafe { image.0.at(0xf2fc).word() } == 0 {
            unsafe { image.0.put(source) };
        } else if unsafe { image.0.at(0xf304).word() } == 1 {
            unsafe { image.0.at(24).put(2) };
        }
        unsafe { image.0.at(0x424).put(0); image.0.at(0x428).put(0) };
    }
    if thumbnail != 0 {
        unsafe { initialize_input(image, 0x19c, ImageInput(Address(thumbnail)), CompressionKind::Thumbnail, 0, 160, 120, 10, 1, 0) };
    }
    if primary != 0 {
        let input = ImageInput(Address(primary));
        unsafe { initialize_input(image, 0x54, input, CompressionKind::Primary, kind, input.0.word(), input.0.at(8).word(), scale as i32, order, flags) };
    }
    if secondary != 0 {
        let input = ImageInput(Address(secondary));
        unsafe { initialize_input(image, 0x2f4, input, CompressionKind::Secondary, 1, input.0.word(), input.0.at(8).word(), 10, order, flags) };
        let alignment = unsafe { input.0.at(24).word() };
        unsafe { image.0.at(0x420).put(alignment) };
        if alignment & 63 != 0 { unsafe { image.0.at(0x420).put(alignment.wrapping_add(64 - (alignment & 63))) }; }
    }
    if unsafe { image.0.at(0xf2fc).word() } == 0 {
        unsafe { image.0.at(0x2c8).put(image.0.at(0xf4).word()); image.0.at(0x2cc).put(image.0.at(0xf8).word()); image.0.at(0x2d4).put(Address(primary).at(12).word()) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_initialize_stream(image: u32, source: u32, owner: u32, thumbnail: u32, primary: u32, secondary: u32, scale: i32, order: i32, flags: i32, kind: i32) {
    unsafe { xm1_pipeline_image_setup_descriptors(image, source, thumbnail, primary, secondary, scale as u32, order as u32, flags as u32, kind as u32, 1); copy_bytes(Address(image).at(28), Address(owner), 28) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_calculate_metadata_limit(limits: u32, width: u32, height: u32, quality: i32, slot: i8) {
    let limits = Address(limits);
    if unsafe { limits.at(16).word() } != 0 { return; }
    let entry = quality.wrapping_mul(2).wrapping_add(i32::from(slot)) as u32;
    let scale = u32::from(unsafe { Address(0x014df3b0).at(entry.wrapping_mul(2)).byte() });
    unsafe { limits.at(16).put(scale.wrapping_mul(width).wrapping_mul(height) / 80) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_select_compression_tables(kind: i32, width: u32, height: u32, slot: i8, _unused: i32, limits: *mut u32, geometry: *mut u32, parameters: *mut u32) -> i32 {
    unsafe { xm1_pipeline_image_initialize_quality_tables() };
    let profile = unsafe { xm1_pipeline_image_find_quality_profile(width, height) };
    if profile == 0 || slot > 1 { fatal(); }
    let entry = profile.wrapping_mul(2).wrapping_add(i32::from(slot) as u32);
    let table = if (kind.wrapping_sub(2) as u32) <= 1 { 0x014ded70u32 } else { 0x014df450u32 };
    let selected = table.wrapping_add(entry.wrapping_mul(20));
    unsafe { write_volatile(limits, selected); write_volatile(geometry, 0x014de9b0u32.wrapping_add(entry.wrapping_mul(12))); xm1_pipeline_image_calculate_metadata_limit(selected, width, height, profile as i32, slot); write_volatile(parameters, 0x011dc39c) };
    0
}

#[repr(C)]
struct RequestCode { message: i16, reserved: i16, code: i32 }
#[repr(C)]
struct RequestHandler { code: i32, callback: Option<unsafe extern "C" fn(u32, i32, i32)> }
#[repr(transparent)]
struct HandlerList(*const RequestHandler);
unsafe impl Sync for HandlerList {}
static IMAGE_CODES: [RequestCode; 9] = [
    RequestCode { message: 0x2b00, reserved: 0, code: 5 },
    RequestCode { message: 0x2b06, reserved: 0, code: 0x19 },
    RequestCode { message: 0xab06u16 as i16, reserved: 0, code: 0x1a },
    RequestCode { message: 0x2b07, reserved: 0, code: 0x1b },
    RequestCode { message: 0xab07u16 as i16, reserved: 0, code: 0x1c },
    RequestCode { message: 0x2b08, reserved: 0, code: 0x1d },
    RequestCode { message: 0xab08u16 as i16, reserved: 0, code: 0x1e },
    RequestCode { message: 0x2b10, reserved: 0, code: 0x16 },
    RequestCode { message: -1, reserved: 0, code: -1 },
];
static IMAGE_IDLE: [RequestHandler; 2] = [RequestHandler { code: 5, callback: Some(image_dispatch_callback) }, RequestHandler { code: 0x5b, callback: None }];
static IMAGE_PRIMARY: [RequestHandler; 2] = [RequestHandler { code: 0x16, callback: Some(image_primary_callback) }, RequestHandler { code: 0x5b, callback: None }];
static IMAGE_HEADER: [RequestHandler; 2] = [RequestHandler { code: 0x16, callback: Some(image_header_callback) }, RequestHandler { code: 0x5b, callback: None }];
static IMAGE_SECONDARY: [RequestHandler; 2] = [RequestHandler { code: 0x16, callback: Some(image_secondary_callback) }, RequestHandler { code: 0x5b, callback: None }];
static IMAGE_HANDLERS: [HandlerList; 4] = [HandlerList(IMAGE_IDLE.as_ptr()), HandlerList(IMAGE_PRIMARY.as_ptr()), HandlerList(IMAGE_HEADER.as_ptr()), HandlerList(IMAGE_SECONDARY.as_ptr())];
static METADATA_CODES: [RequestCode; 3] = [RequestCode { message: 0x2b05, reserved: 0, code: 0x18 }, RequestCode { message: 0x2b10, reserved: 0, code: 0x16 }, RequestCode { message: -1, reserved: 0, code: -1 }];
static METADATA_IDLE: [RequestHandler; 2] = [RequestHandler { code: 0x18, callback: Some(metadata_begin_callback) }, RequestHandler { code: 0x5b, callback: None }];
static METADATA_BUSY: [RequestHandler; 2] = [RequestHandler { code: 0x16, callback: Some(metadata_completed_callback) }, RequestHandler { code: 0x5b, callback: None }];
static METADATA_HANDLERS: [HandlerList; 2] = [HandlerList(METADATA_IDLE.as_ptr()), HandlerList(METADATA_BUSY.as_ptr())];

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_start_request(id: u32, image: u32, completion: u32, mode: i32) {
    let queue = Address(0x014ddb74);
    unsafe { xm1_pipeline_image_request_initialize(queue.0, id, 0, 1, 0, IMAGE_CODES.as_ptr() as u32, IMAGE_HANDLERS.as_ptr() as u32, queue.0); queue.at(32).put(completion); queue.at(40).put(image); queue.at(44).put(mode as u32) };
    let image = Address(image);
    if mode == 0 { unsafe { image.at(0xf2fc).put(0) }; }
    else if unsafe { image.at(0xf2fc).word() } == 1 {
        let phase = match mode { 3 => 2, 4 => 3, _ => 4 };
        unsafe { xm1_pipeline_image_select_raw_buffer(phase, image.0) };
    }
    unsafe { xm1_pipeline_image_request_send(queue.0, 0x2b00, 0, 0, 0) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_start_compact_request(id: u32, image: u32, completion: u32, mode: i32, compact: i32) {
    unsafe { Address(0x014ddba4).put(0); Address(image).at(0xf2f0).put(compact as u32) };
    if mode == 4 { unsafe { Address(image).at(0x424).put(1) }; }
    unsafe { xm1_pipeline_image_start_request(id, image, completion, mode) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_start_secondary_request(id: u32, image: u32, completion: u32) {
    unsafe { Address(image).at(0x424).put(1); xm1_pipeline_image_start_request(id, image, completion, 4) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_request_halt() -> ! { fatal() }
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_request_completed() {
    unsafe { xm1_pipeline_image_request_send(0x014de980, 0x2b10, 0, 0, 0) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_stream_signal_completion(status: i32) {
    unsafe { xm1_pipeline_image_request_send(0x014ddb74, 0x2b10, status as u32, 0, 0) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_start_compression(priority: u32, request: u32, callback: u32) {
    let state = Address(0x014de980);
    unsafe { state.at(40).put(request); xm1_pipeline_image_request_initialize(state.0, priority, 0, 0x11, 0, METADATA_CODES.as_ptr() as u32, METADATA_HANDLERS.as_ptr() as u32, state.0) };
    let request = Address(unsafe { state.at(40).word() });
    unsafe { request.at(8).put(0); clear_bytes(request.at(32), 128); state.at(32).put(callback); xm1_pipeline_image_request_send(state.0, 0x2b05, 0, 0, 0) };
}

unsafe extern "C" fn image_dispatch_callback(task: u32, _code: i32, _mode: i32) { unsafe { xm1_pipeline_image_dispatch_job(task) }; }
unsafe extern "C" fn image_primary_callback(task: u32, _code: i32, _mode: i32) { unsafe { xm1_pipeline_image_primary_completed(task) }; }
unsafe extern "C" fn image_header_callback(task: u32, _code: i32, _mode: i32) { unsafe { xm1_pipeline_image_header_completed(task) }; }
unsafe extern "C" fn image_secondary_callback(task: u32, _code: i32, _mode: i32) { unsafe { xm1_pipeline_image_secondary_completed(task) }; }
unsafe extern "C" fn metadata_begin_callback(task: u32, _code: i32, _mode: i32) { unsafe { xm1_pipeline_image_begin_compression(task) }; }
unsafe extern "C" fn metadata_completed_callback(task: u32, _code: i32, _mode: i32) { unsafe { xm1_pipeline_image_compression_completed(task) }; }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_set_stream_active(active: u32) {
    unsafe { Address(0x014ddba4).put(active) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_dispatch_job(task: u32) {
    let task = Address(task);
    let owner = Address(unsafe { task.at(28).word() });
    let image = Address(unsafe { owner.at(40).word() });
    let mode = unsafe { owner.at(44).word() };
    let completion = xm1_pipeline_image_stream_signal_completion as *const () as u32;
    match mode {
        0 if unsafe { image.at(0xf2f0).word() } == 1 => {
            unsafe { task.at(12).put(2); xm1_pipeline_image_stream_signal_completion(0) };
            return;
        }
        2 => {
            unsafe { xm1_pipeline_image_start_compression(2, image.at(0x54).0, completion); task.at(12).put(2) };
            return;
        }
        4 => {
            if unsafe { image.at(0xf2f0).word() } == 1 {
                unsafe { task.at(12).put(3); xm1_pipeline_image_stream_signal_completion(0) };
            } else {
                unsafe { xm1_pipeline_image_start_compression(2, image.at(0x2f4).0, completion); task.at(12).put(3) };
            }
            return;
        }
        _ => {}
    }
    unsafe { xm1_pipeline_image_configure_stream_output(image.0); xm1_pipeline_image_start_compression(2, image.at(0x19c).0, completion); task.at(12).put(1) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_pad_primary(image: u32) {
    let image = Address(image);
    let size = unsafe { image.at(0x254).word() };
    let mut total = size.wrapping_add(unsafe { image.at(8).word() });
    let remainder = ((unsafe { image.word() } & 63).wrapping_add(total)) & 0xffff;
    unsafe { image.at(0x180).put(image.at(0x248).word()) };
    let output_size = if remainder != 0 {
        let padding = unsafe { image.at(24).word() }.wrapping_add(0xffc0).wrapping_sub(remainder);
        unsafe { xm1_pipeline_image_fill_jpeg_padding(image.at(0x248).word().wrapping_add(size), padding, 0) };
        total = total.wrapping_add(padding);
        padding.wrapping_add(size)
    } else { size };
    unsafe { image.at(0x188).put(output_size); xm1_pipeline_image_finish_exif(total, size & 0xffff) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_primary_completed(task: u32) {
    let task = Address(task);
    let owner = Address(unsafe { task.at(28).word() });
    let status = unsafe { Address(task.at(24).word()).at(4).word() };
    if status == u32::MAX {
        let completion = Completion(unsafe { owner.at(32).word() });
        unsafe { task.at(12).put(0); xm1_pipeline_image_request_unregister(task.0); xm1_pipeline_image_diagnostic_event(1, 0x81); completion.call(-1) };
    } else {
        let image = Address(unsafe { owner.at(40).word() });
        unsafe { image.at(0x428).put(image.at(0x428).word() | 2) };
        if unsafe { owner.at(44).word() } == 0 {
            unsafe { xm1_pipeline_image_start_compression(2, image.at(0x54).0, xm1_pipeline_image_stream_signal_completion as *const () as u32); task.at(12).put(2) };
        }
    }
    let mode = unsafe { owner.at(44).word() };
    if mode == 1 || mode == 3 {
        let image = Address(unsafe { owner.at(40).word() });
        if mode == 1 { unsafe { xm1_pipeline_image_relocate_jpeg(image.0, 0) }; }
        if unsafe { owner.at(44).word() } == 3 && unsafe { image.at(0xf2fc).word() } == 1 {
            unsafe { xm1_pipeline_image_pad_primary(image.0); xm1_pipeline_image_advance_raw_output(2, image.0) };
        }
        let completion = Completion(unsafe { owner.at(32).word() });
        unsafe { xm1_pipeline_image_request_unregister(task.0); completion.call(0) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_header_completed(task: u32) {
    let task = Address(task);
    let owner = Address(unsafe { task.at(28).word() });
    if unsafe { Address(task.at(24).word()).at(4).word() } == u32::MAX {
        let completion = Completion(unsafe { owner.at(32).word() });
        unsafe { task.at(12).put(0); xm1_pipeline_image_request_unregister(task.0); xm1_pipeline_image_diagnostic_event(1, 0x82); completion.call(-1) };
        return;
    }
    let image = Address(unsafe { owner.at(40).word() });
    unsafe { image.at(0x428).put(image.at(0x428).word() | 4) };
    if unsafe { owner.at(44).word() } == 0 { unsafe { xm1_pipeline_image_relocate_jpeg(image.0, 0) }; }
    if unsafe { owner.at(44).word() } == 2 {
        let image = Address(unsafe { owner.at(40).word() });
        if unsafe { image.at(0xf2fc).word() } == 1 {
            unsafe {
                Address(image.at(0x100).word()).put_half(0);
                image.at(0x38).put(image.at(0x100).word().wrapping_add(2));
                image.at(0x40).put(image.at(0x10c).word().wrapping_sub(2));
                xm1_pipeline_image_advance_raw_output(4, image.0);
            }
        } else if unsafe { image.at(0xf300).byte() } == 0 {
            unsafe { Address(image.at(0x100).word()).put_half(0) };
        }
    }
    let completion = Completion(unsafe { owner.at(32).word() });
    unsafe { xm1_pipeline_image_request_unregister(task.0); completion.call(0) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_secondary_completed(task: u32) {
    let task = Address(task);
    let owner = Address(unsafe { task.at(28).word() });
    let image = Address(unsafe { owner.at(40).word() });
    unsafe { image.at(0x428).put(image.at(0x428).word() | 8) };
    if unsafe { image.at(0xf2f4).word() } == 0 {
        let current = unsafe { xm1_pipeline_image_current_task() };
        unsafe { xm1_pipeline_image_configure_task(current, 11) };
        let size = unsafe { xm1_pipeline_image_write_chunked_segments(image.0) };
        if unsafe { image.at(0x424).word() } == 1 && unsafe { image.at(0x428).word() } & 0x13 == 3 {
            unsafe { xm1_pipeline_image_align_jpeg(image.0) };
        }
        unsafe { image.at(0x2d8).put(image.at(0x420).word()); image.at(0x2e0).put(size) };
    }
    let current = unsafe { xm1_pipeline_image_current_task() };
    match current {
        0x1d => { unsafe { xm1_pipeline_image_configure_task(0x1d, 6) }; }
        0x1e => { unsafe { xm1_pipeline_image_configure_task(0x1e, 3) }; }
        0x1b => { unsafe { xm1_pipeline_image_configure_task(0x1b, 9) }; }
        _ => { let current = unsafe { xm1_pipeline_image_current_task() }; unsafe { xm1_develop_append_stream_text(current & 255, 0x011d4fac) }; }
    }
    let image = unsafe { owner.at(40).word() };
    if unsafe { Address(image).at(0xf2fc).word() } == 1 { unsafe { xm1_pipeline_image_advance_raw_output(3, image) }; }
    unsafe { xm1_pipeline_image_request_unregister(task.0) };
    let completion = Completion(unsafe { owner.at(32).word() });
    unsafe { completion.call(0) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_begin_compression(event: u32) {
    let event = Address(event);
    let request = Address(unsafe { Address(event.at(28).word()).at(40).word() });
    unsafe { request.at(8).put(1) };
    let input = Address(unsafe { request.at(16).word() });
    let desired = unsafe { request.at(0xb0).word() };
    let input_size = unsafe { input.at(16).word() };
    if unsafe { request.at(12).word() } == 1 {
        if desired == 0 { unsafe { request.at(0xb0).put(input_size.wrapping_add(((input_size as i32) >> 2) as u32).wrapping_add(7) & !7) }; }
        let target = unsafe { request.at(0xb0).word() }.wrapping_mul(9) / 10;
        unsafe { input.at(4).put(target); request.at(0xb4).put(target) };
    } else {
        if desired == 0 {
            unsafe { request.at(0xb0).put(input_size.wrapping_mul(2).wrapping_add(7) & !7); request.at(0xb4).put(input_size) };
        } else {
            let target = desired.wrapping_mul(9) / 10;
            unsafe { request.at(0xb4).put(input_size.min(target)) };
        }
        unsafe { input.at(4).put(input_size) };
    }
    let target = unsafe { input.at(4).word() } as i32;
    unsafe { Address(request.at(20).word()).put(target.wrapping_mul(95).wrapping_div(100) as u32); xm1_pipeline_image_submit_compression(request.0); event.at(12).put(1) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_retry_compression(request: u32) -> i32 {
    let request = Address(request);
    let index = unsafe { request.at(8).word() };
    let size = unsafe { request.at(0xb8).word() };
    unsafe { request.at(0x24u32.wrapping_add(index.wrapping_mul(8))).put(size) };
    let error = unsafe { request.at(0xbc).word() };
    let attempts = unsafe { request.at(4).word() } as i32;
    let mut retry = error != 0;
    if error == 0 && attempts < 0 {
        let target = unsafe { Address(request.at(20).word()).word() } as i32;
        let delta = target.wrapping_sub(size as i32).wrapping_abs();
        retry = delta > target.wrapping_mul(5).wrapping_div(100);
    }
    if !retry { return 0; }
    if index < attempts.wrapping_abs() as u32 {
        if size == 0 { unsafe { request.at(0x24u32.wrapping_add(index.wrapping_mul(8))).put(request.at(0xb0).word()) }; }
        unsafe { request.at(8).put(index.wrapping_add(1)); xm1_pipeline_image_submit_compression(request.0) };
        return 1;
    }
    if attempts < 0 && error == 0 { unsafe { request.at(0x24u32.wrapping_add(index.wrapping_add(1).wrapping_mul(8))).put(u32::MAX) }; }
    if error != 0 { -1 } else { 0 }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_complete_compression(request: u32) -> i32 {
    let request = Address(request);
    if unsafe { request.at(0xc0).word() } == 1 { unsafe { xm1_pipeline_image_warp_cancel() }; }
    unsafe { xm1_pipeline_image_channel8_finish() };
    if unsafe { request.at(0xc0).word() } == 1 { unsafe { xm1_pipeline_image_warp_release() }; }
    unsafe { xm1_pipeline_image_channel8_release(); xm1_pipeline_image_retry_compression(request.0) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_compression_completed(event: u32) {
    let event = Address(event);
    let state = Address(unsafe { event.at(28).word() });
    if unsafe { Address(event.at(24).word()).at(4).word() } == 1 { fatal(); }
    let result = unsafe { xm1_pipeline_image_complete_compression(state.at(40).word()) };
    if result == 1 { return; }
    let callback = Completion(unsafe { state.at(32).word() });
    unsafe { event.at(12).put(0) };
    if result == -1 {
        unsafe { xm1_pipeline_image_diagnostic_event(1, 0x80); xm1_pipeline_image_request_unregister(event.0); callback.call(-1) };
    } else {
        unsafe { xm1_pipeline_image_request_unregister(event.0); callback.call(0) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_power_scale(base: i32, exponent: i32) -> u32 {
    let x = i64::from(base).wrapping_mul(8).wrapping_sub(8192);
    let e = i64::from(exponent).wrapping_mul(8);
    let mut power = x;
    let mut coefficient = e;
    let mut total = coefficient.wrapping_mul(power);
    for degree in 2i64..=6 {
        power = power.wrapping_mul(x) >> 13;
        coefficient = coefficient.wrapping_mul(e.wrapping_sub((degree - 1) * 8192)).wrapping_div(degree) >> 13;
        total = total.wrapping_add(coefficient.wrapping_mul(power));
    }
    ((total >> 13).wrapping_add(8192) >> 3).max(3) as u32
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_compute_scale(numerator: i32, denominator: i32, extent: i32, exponent: i32, offset: i32, enforce_minimum: i32) -> i32 {
    let mut ratio = (i64::from(numerator).wrapping_mul(1024) / i64::from(denominator)) as i32;
    if ratio > 0xb1f { ratio = 0xb20; }
    let divisor = unsafe { xm1_pipeline_image_power_scale(ratio, exponent) } as i32;
    let result = extent.wrapping_shl(10).wrapping_div(divisor).wrapping_add(offset);
    if enforce_minimum == 0 { result.max(1) } else {
        let minimum = unsafe { super::buffers::xm1_buffers_configuration_byte(0x261) } as i32;
        if result < minimum { (unsafe { super::buffers::xm1_buffers_configuration_byte(0x261) }) as i32 } else { result }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_compute_mode_scale(mode: u32, denominator: i32, extent: i32, parameters: u32) -> i32 {
    if mode <= 1 { fatal(); }
    let parameters = Address(parameters);
    unsafe { xm1_pipeline_image_compute_scale(parameters.word() as i32, denominator, extent, parameters.at(4).word() as i32, parameters.at(8).word() as i32, 0) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_request_value(input: u32, parameters: u32, size: i32, quality: i32, index: i32) -> i32 {
    if index == 1 { unsafe { Address(input).word() as i32 } } else { unsafe { xm1_pipeline_image_compute_mode_scale(index as u32, size, quality, parameters) } }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scale_quantization(source: u32, destination: u32, first_scale: i32, second_scale: u32) {
    let source = Address(source);
    let destination = Address(destination);
    unsafe { destination.put_byte(1) };
    let scale = (first_scale as u32).wrapping_mul(second_scale);
    for index in 1..64 {
        let order = u32::from(unsafe { Address(0x00d99324).at(index).byte() });
        let coefficient = unsafe { source.at(order).byte() };
        let scaled = (scale.wrapping_mul(u32::from(coefficient)) >> 10).clamp(1, 255);
        unsafe { Address(0x014c574c).at(index).put_byte(coefficient); destination.at(index).put_byte(scaled as u8) };
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_prepare_compression_tables(channel: u32, quality: i32, option: u32, alternate: u32) {
    let channel = Address(channel);
    if unsafe { channel.at(0x24).word() } != 1 { return; }
    let secondary = if alternate == 0 { 0x00d992e4 } else { 0x00d99264 };
    unsafe {
        xm1_pipeline_image_scale_quantization(0x00d992a4, 0x014de940, quality, option);
        xm1_pipeline_image_scale_quantization(secondary, 0x014de900, quality, option);
        channel.at(0x28).put(0x014de940);
        channel.at(0x2c).put(0x014de900);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_submit_compression(request: u32) {
    let request = Address(request);
    let index = unsafe { request.at(8).word() };
    let previous = request.at(0x20u32.wrapping_add(index.wrapping_sub(1).wrapping_mul(8)));
    let quality = unsafe { xm1_pipeline_image_metadata_request_value(request.at(16).word(), request.at(20).word(), previous.at(4).word() as i32, previous.word() as i32, index as i32) };
    unsafe { request.at(0x20u32.wrapping_add(index.wrapping_mul(8))).put(quality as u32); xm1_pipeline_image_prepare_compression_tables(request.at(0xa0).0, quality, request.at(24).word(), request.at(28).word()) };
    if unsafe { request.at(0xc0).word() } == 1 { unsafe { xm1_pipeline_image_warp_initialize() }; }
    unsafe { xm1_pipeline_image_channel8_acquire() };
    if unsafe { request.at(0xc0).word() } == 1 { unsafe { xm1_pipeline_image_warp_configure(request.at(0xdc).0) }; }
    unsafe { xm1_pipeline_image_channel8_configure(request.at(0xa0).0) };
    if unsafe { request.at(0xc0).word() } == 1 { unsafe { xm1_pipeline_image_warp_start() }; }
    unsafe { xm1_pipeline_image_channel8_submit(request.at(0xa0).0) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_preview_enabled() -> u32 {
    let enabled = Address(0x00d99364);
    if unsafe { enabled.byte() } == 255 { unsafe { enabled.put_byte(1) }; }
    u32::from(unsafe { enabled.byte() })
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_classify_aspect(width: u32, height: u32) -> i32 {
    if width.wrapping_mul(3) == height.wrapping_mul(4) { 0 } else if height >= width { 2 } else { 1 }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_fill_pixel_pairs(destination: u32, width: u32, height: u32, pixels: u32) {
    for index in 0..width.wrapping_mul(height) >> 1 { unsafe { Address(destination).at(index.wrapping_mul(4)).put(pixels) }; }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scale_preview_pairs(source: u32, output: u32, source_width: u32, source_padding: u32, source_height: u16, output_width: u16, output_height: u16, row_count: u16) {
    if row_count == 0 { return; }
    let pairs = u32::from(output_width >> 1);
    let horizontal = ((source_width >> 1) << 4) / pairs;
    let vertical = (u32::from(source_height) << 4) / u32::from(output_height);
    let top = (120u32.wrapping_sub(u32::from(output_height)) >> 1) & 0x7fff;
    let difference = 160u32.wrapping_sub(u32::from(output_width));
    let left = (difference.wrapping_add(difference >> 31) >> 2) & 0x7fff;
    let pitch = source_width.wrapping_add(source_padding);
    let mut accumulator = vertical;
    let mut sample_row = vertical;
    for row in 0..u32::from(row_count) {
        let origin = Address(source).at((horizontal >> 6).wrapping_add(pitch.wrapping_mul(sample_row >> 4) >> 1).wrapping_mul(4));
        let destination = Address(output).at(left.wrapping_add(top.wrapping_add(row).wrapping_mul(80)).wrapping_mul(4));
        let mut sample = origin;
        let mut horizontal_accumulator = horizontal;
        for pair in 0..pairs {
            let destination = destination.at(pair * 4);
            unsafe {
                destination.put_byte(sample.byte());
                destination.at(1).put_byte(sample.at((horizontal >> 5) * 4).byte());
                destination.at(2).put_byte(sample.at(2).byte());
                destination.at(3).put_byte(sample.at(3).byte());
            }
            sample = origin.at((horizontal_accumulator >> 4).wrapping_mul(4));
            horizontal_accumulator = horizontal_accumulator.wrapping_add(horizontal);
        }
        sample_row = accumulator;
        accumulator = accumulator.wrapping_add(vertical);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_scale_preview(source: u32, output: u32, source_width: u32, source_padding: u32, source_height: u16, output_width: u16, output_height: u16) {
    unsafe { xm1_pipeline_image_scale_preview_pairs(source, output, source_width, source_padding, source_height, output_width, output_height, output_height) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_make_preview(source: u32, output: u32, width: u32, padding: u32, height: u16) {
    let fill = if unsafe { xm1_pipeline_image_preview_enabled() } == 1 { 0x80800000 } else { 0x8080 };
    let aspect = unsafe { xm1_pipeline_image_classify_aspect(width, u32::from(height)) };
    let (output_width, output_height) = match aspect {
        1 => {
            unsafe { xm1_pipeline_image_fill_pixel_pairs(output, 160, 120, fill) };
            (160, (i32::from(height).wrapping_mul(160) / width as i32) as u16)
        }
        2 => {
            unsafe { xm1_pipeline_image_fill_pixel_pairs(output, 160, 120, fill) };
            (((width as i32).wrapping_mul(120) / i32::from(height)) as u16, 120)
        }
        _ => (160, 120),
    };
    unsafe { xm1_pipeline_image_scale_preview(source, output, width, padding, height, output_width, output_height) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_configure_stream_output(image: u32) {
    let image = Address(image);
    if unsafe { image.at(0x25c).word() } == 0 {
        unsafe { xm1_pipeline_image_make_preview(image.at(0x2d4).word(), image.at(0x244).word(), image.at(0x2c8).word() & 0xffff, image.at(0x2d0).word() & 0xffff, image.at(0x2cc).word() as u16) };
    }
}

unsafe extern "C" {
    fn xm1_pipeline_image_wait_reserved(handle: u32, timeout: u32) -> i32;
    fn xm1_pipeline_image_signal_reserved(handle: u32) -> i32;
    fn xm1_pipeline_image_route_request(selector: u32, slot: u32, level: u32) -> i32;
    fn xm1_pipeline_image_route_acquire(index: u32, output: u32) -> i32;
    fn xm1_pipeline_image_route_release(handle: u32) -> i32;
    fn xm1_pipeline_image_interrupt_acquire(source: u32, callback: u32, output: u32) -> i32;
    fn xm1_pipeline_image_interrupt_release(handle: u32) -> i32;
    fn xm1_encoder_cache(address: u32, length: u32);
    fn xm1_pipeline_image_signal_current_zero() -> i32;
    fn xm1_pipeline_image_wait_all(mask: u32, result: *mut u32, timeout: i32) -> i32;
    fn xm1_pipeline_image_send_notification(task: u32, mask: u32) -> i32;
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_channel8_invalid() -> ! { fatal() }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_channel8_reset() {
    unsafe { Address(0xfff62118).put(1) };
    let status = loop {
        let status = unsafe { Address(0xfff62024).word() };
        if status & 0x10000 != 0 { break status; }
    };
    unsafe { Address(0xfff62024).put(status); Address(0xfff62000).put(0x100) };
    while unsafe { Address(0xfff62000).word() } & 0x100 != 0 {}
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_channel8_request_resource(output: u32) -> i32 {
    unsafe { xm1_pipeline_image_route_request(0x508, 25, 10); xm1_pipeline_image_route_request(0x609, 25, 8); xm1_pipeline_image_route_acquire(0x1d, output) }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_channel8_release_resource(handle: u32) -> i32 {
    let result = unsafe { xm1_pipeline_image_route_release(handle) };
    unsafe { xm1_pipeline_image_route_request(0x508, 25, 0); xm1_pipeline_image_route_request(0x609, 25, 0) };
    result
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_channel8_acquire() {
    if unsafe { xm1_pipeline_image_wait_reserved(8, u32::MAX) } != 0 { fatal(); }
    if unsafe { Address(0x014c56ec).word() } == 0 && unsafe { xm1_pipeline_image_channel8_request_resource(0x014c56ec) } != 0 { fatal(); }
    unsafe { xm1_pipeline_image_channel8_reset() };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_channel8_release() {
    let handle = unsafe { Address(0x014c56ec).word() };
    if handle != 0 { unsafe { xm1_pipeline_image_channel8_release_resource(handle); Address(0x014c56ec).put(0) }; }
    let result = unsafe { xm1_pipeline_image_signal_reserved(8) };
    if result != 0 && result != -43 { fatal(); }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_channel8_start() {
    unsafe { Address(0xfff62000).put(Address(0xfff62000).word() | 1) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_channel8_finish() {
    unsafe { xm1_pipeline_image_channel8_reset() };
    let handle = unsafe { Address(0x014c56e8).word() };
    if handle != 0 { unsafe { xm1_pipeline_image_interrupt_release(handle); Address(0x014c56e8).put(0) }; }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_channel8_submit(parameters: u32) {
    let result = unsafe { xm1_pipeline_image_interrupt_acquire(0x49, xm1_pipeline_image_channel8_completed as *const () as u32, 0x014c56e8) };
    if result != 0 {
        let parameters = Address(parameters);
        unsafe { parameters.at(28).put(3) };
        let callback = unsafe { parameters.at(56).word() };
        if callback != 0 {
            let callback: unsafe extern "C" fn() = unsafe { core::mem::transmute(callback) };
            unsafe { callback() };
        }
    }
    unsafe { xm1_encoder_cache(0, 0); xm1_pipeline_image_channel8_start() };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_channel8_completed() {
    let status = unsafe { Address(0xfff62024).word() };
    unsafe { Address(0xfff62024).put(status) };
    let parameters = Address(unsafe { Address(0x014c578c).word() });
    if status & 16 != 0 {
        unsafe { parameters.at(28).put(0) };
        let size = unsafe { Address(0xfff62094).word() };
        unsafe { parameters.at(24).put(size) };
        if size > unsafe { parameters.at(20).word() } { unsafe { parameters.at(28).put(2) }; }
    } else {
        unsafe { parameters.at(28).put(2); parameters.at(24).put(0) };
    }
    let parameters = Address(unsafe { Address(0x014c578c).word() });
    let callback = unsafe { parameters.at(56).word() };
    if callback != 0 {
        let callback: unsafe extern "C" fn() = unsafe { core::mem::transmute(callback) };
        unsafe { callback() };
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_channel8_configure(parameters: u32) {
    let parameters = Address(parameters);
    let configuration = Address(0x014c5790);
    unsafe { Address(0x014c578c).put(parameters.0) };
    for (destination, source) in [(0, 8), (4, 0), (8, 4), (12, 12), (16, 16), (48, 52)] {
        unsafe { configuration.at(destination).put(parameters.at(source).word()) };
    }
    unsafe { configuration.at(20).put(1); configuration.at(24).put(1) };
    for offset in [36, 40, 44] { unsafe { configuration.at(offset).put(0) }; }
    if unsafe { parameters.at(36).word() } == 1 {
        unsafe { configuration.at(52).put(1); copy_bytes(configuration.at(56), Address(parameters.at(40).word()), 64) };
        let parameters = Address(unsafe { Address(0x014c578c).word() });
        unsafe { copy_bytes(configuration.at(120), Address(parameters.at(44).word()), 64) };
    } else {
        unsafe { configuration.at(52).put(0); clear_bytes(configuration.at(56), 128) };
    }
    let parameters = Address(unsafe { Address(0x014c578c).word() });
    let mode = unsafe { parameters.at(48).word() };
    unsafe { configuration.at(28).put(mode); configuration.at(32).put(mode) };
    for offset in [0xb8, 0xbc, 0xc0, 0xc4, 0xcc, 0xd0, 0xd4, 0xdc] { unsafe { configuration.at(offset).put(0) }; }
    unsafe { configuration.at(0xc8).put(u32::from(parameters.at(32).word() != 0)); configuration.at(0xd8).put(1) };
    for offset in [0xe0, 0xe4, 0xe8, 0xec, 0xf0, 0xf4] { unsafe { configuration.at(offset).put(1) }; }
    unsafe { xm1_pipeline_image_channel8_publish(configuration.0) };
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_metadata_start_channel8(parameters: u32) {
    unsafe { xm1_pipeline_image_channel8_configure(parameters) };
}

unsafe fn bounded_word(configuration: Address, offset: u32, maximum: u32) -> u32 {
    let value = unsafe { configuration.at(offset).word() };
    if value > maximum { fatal(); }
    value
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_channel8_publish(configuration: u32) {
    let configuration = Address(configuration);
    unsafe { Address(0xfff62000).put((configuration.at(24).word() & 1) << 2) };
    let mut interrupts = (unsafe { configuration.at(0xd4).word() } & 1) << 4;
    for index in 0..8 {
        interrupts |= (unsafe { configuration.at(0xd8 + index * 4).word() } & 1) << (16 + index);
    }
    unsafe { Address(0xfff62028).put(interrupts); Address(0xfff62040).put(configuration.at(20).word() & 3) };
    let width = unsafe { bounded_word(configuration, 4, 0xffff) };
    let height = unsafe { bounded_word(configuration, 8, 0xffff) };
    unsafe { Address(0xfff62044).put(width | (height << 16)); Address(0xfff62048).put((configuration.at(28).word() & 15) | ((configuration.at(32).word() & 15) << 4)) };
    let crop = unsafe { bounded_word(configuration, 36, 1) };
    unsafe { Address(0xfff6204c).put(crop) };
    let offset = unsafe { bounded_word(configuration, 44, 255) };
    unsafe { Address(0xfff62050).put((configuration.at(40).word() & 1) | (offset << 16)) };
    let stride = unsafe { bounded_word(configuration, 48, 0xffff) };
    unsafe { Address(0xfff62080).put(stride) };
    for (source, destination) in [(0, 0xfff62100), (12, 0xfff6210c), (16, 0xfff62110)] {
        let address = unsafe { configuration.at(source).word() };
        if address & 7 != 0 { fatal(); }
        unsafe { Address(destination).put(address) };
    }
    unsafe {
        Address(0xfff6211c).put((configuration.at(0xb8).word() & 15) | ((configuration.at(0xbc).word() & 1) << 4) | ((configuration.at(0xc0).word() & 1) << 5));
        Address(0xfff62120).put((configuration.at(0xc4).word() & 1) | ((configuration.at(0xc8).word() & 1) << 1));
    }
    let first = unsafe { bounded_word(configuration, 0xcc, 0xffff) };
    unsafe { Address(0xfff62124).put(first) };
    let second = unsafe { bounded_word(configuration, 0xd0, 0xffff) };
    unsafe { Address(0xfff62128).put(second) };
    let tables = unsafe { configuration.at(52).word() };
    unsafe { Address(0xfff62084).put(if tables == 1 { 0x100 } else { 0 }) };
    if tables == 1 {
        for table in [56, 120] {
            for index in 0..16 {
                let coefficients = configuration.at(table + index * 4);
                let word = (u32::from(unsafe { coefficients.byte() }) << 24)
                    | (u32::from(unsafe { coefficients.at(1).byte() }) << 16)
                    | (u32::from(unsafe { coefficients.at(2).byte() }) << 8)
                    | u32::from(unsafe { coefficients.at(3).byte() });
                unsafe { Address(0xfff6208c).put(word) };
            }
        }
    }
    unsafe { Address(0xfff62084).put(tables & 1) };
}

unsafe extern "C" {
    fn xm1_pipeline_image_driver_allocate(slot: u32, capacity: u32, handles: u32) -> i32;
    fn xm1_pipeline_image_driver_find(handle: u32, capacity: u32, handles: u32, output: *mut u32) -> i32;
    fn xm1_pipeline_image_driver_clear(slot: u32, capacity: u32, handles: u32) -> i32;
    fn xm1_encoder_save_irq() -> u32;
    fn xm1_encoder_restore_irq(saved: u32);
}

#[repr(C)]
pub struct DmaProgram {
    control: u32,
    register34: u32,
    register30: u32,
    source: u32,
    destination: u32,
    bytes: u32,
    register0c: core::mem::MaybeUninit<u32>,
    register10: core::mem::MaybeUninit<u32>,
    register14: core::mem::MaybeUninit<u32>,
    link: core::mem::MaybeUninit<u32>,
    priority: core::mem::MaybeUninit<u32>,
}
#[repr(C)]
pub struct DmaCompletion {
    argument: u32,
    callback: u32,
}
unsafe fn dma_channel(handle: u32) -> Result<u32, i32> {
    let mut channel = 0;
    if unsafe { xm1_pipeline_image_driver_find(handle, 8, 0x00918bd8, &mut channel) } != 0 { Err(-20) } else { Ok(channel) }
}
fn dma_register(channel: u32, offset: u32) -> Address { Address(0xfff02000u32.wrapping_add(channel.wrapping_mul(64)).wrapping_add(offset)) }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_dma_open(kind: u32, output: *mut u32) -> i32 {
    if kind > 5 { return -30; }
    let channel = unsafe { Address(0x007c86bc).at(kind * 4).word() };
    if unsafe { xm1_pipeline_image_wait_reserved(channel.wrapping_add(11), u32::MAX) } != 0 { return -255; }
    let saved = unsafe { super::buffers::xm1_develop_enter_critical() };
    let result = unsafe { xm1_pipeline_image_driver_allocate(channel, 8, 0x00918bd8) };
    unsafe { super::buffers::xm1_develop_leave_critical(saved) };
    if result != 0 { return -20; }
    unsafe { write_volatile(output, Address(0x00918bd8).at(channel * 4).word()); Address(0x00918b98).at(channel * 4).put(1); Address(0x00918bb8).at(channel * 4).put(kind) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_dma_close(handle: u32) -> i32 {
    let channel = match unsafe { dma_channel(handle) } { Ok(channel) => channel, Err(error) => return error };
    let interrupt = Address(0x00918bf8).at(channel * 4);
    let mut result = 0;
    let registration = unsafe { interrupt.word() };
    if registration != 0 { result = unsafe { xm1_pipeline_image_interrupt_release(registration) }; unsafe { interrupt.put(0) }; }
    unsafe { Address(0x00918bb8).at(channel * 4).put(u32::MAX); Address(0x00918b98).at(channel * 4).put(0); xm1_pipeline_image_driver_clear(channel, 8, 0x00918bd8) };
    if unsafe { xm1_pipeline_image_signal_reserved(channel + 11) } != 0 { return -255; }
    result
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_dma_wait(handle: i32) -> i32 {
    let channel = match unsafe { dma_channel(handle as u32) } { Ok(channel) => channel, Err(error) => return error };
    let state = Address(0x00918b98).at(channel * 4);
    if unsafe { state.word() } != 2 { return -11; }
    while unsafe { dma_register(channel, 0x24).word() } & 1 != 0 {}
    let interrupt = Address(0x00918bf8).at(channel * 4);
    let registration = unsafe { interrupt.word() };
    let result = if registration != 0 {
        let result = unsafe { xm1_pipeline_image_interrupt_release(registration) };
        unsafe { interrupt.put(0) };
        result
    } else {
        0
    };
    unsafe { state.put(1) };
    result
}

unsafe fn physical_word(address: Address) -> u32 {
    let value: u32;
    unsafe { core::arch::asm!("ldr {value}, [{address}]", value = lateout(reg) value, address = in(reg) address.0, options(readonly, nostack, preserves_flags)) };
    value
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_dma_program(handle: u32, program: *const DmaProgram) -> i32 {
    let program = Address(program as u32);
    let mut control = unsafe { program.word() };
    let channel = match unsafe { dma_channel(handle) } { Ok(channel) => channel, Err(error) => return error };
    if control & 0x30 != 0 {
        let saved = unsafe { xm1_encoder_save_irq() };
        let priority_register = Address(if channel < 4 { 0xfff98500 } else { 0xfff98504 });
        let shift = (channel & 3) * 8;
        unsafe { priority_register.put((priority_register.word() & !(31 << shift)) | (physical_word(program.at(40)) << shift)); xm1_encoder_restore_irq(saved) };
        control = (control & !7) | (channel & 7);
    }
    unsafe { dma_register(channel, 0x2c).put(control) };
    if control & 0x80000000 != 0 {
        unsafe { dma_register(channel, 0x38).put(physical_word(program.at(36)) & !3) };
    } else {
        for (input, output) in [(12, 0), (16, 4), (20, 8), (24, 12), (28, 16), (32, 20), (8, 48), (4, 52)] {
            unsafe { dma_register(channel, output).put(physical_word(program.at(input))) };
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_dma_begin(handle: u32, completion: *const DmaCompletion) -> i32 {
    let channel = match unsafe { dma_channel(handle) } { Ok(channel) => channel, Err(error) => return error };
    let status = unsafe { dma_register(channel, 0x24).word() };
    if unsafe { Address(0x00918b98).at(channel * 4).word() } != 1 || status & 5 != 0 { return -12; }
    let completion = unsafe { read_volatile(completion) };
    if completion.callback != 0 {
        let result = unsafe { xm1_pipeline_image_interrupt_acquire(completion.argument, completion.callback, 0x00918bf8 + channel * 4) };
        if result != 0 { return result; }
    }
    unsafe { Address(0x00918b98).at(channel * 4).put(2); dma_register(channel, 0x28).put(8) };
    let control = if unsafe { dma_register(channel, 0x2c).word() } & 0x30 == 0 { 4 } else { 0 };
    unsafe { xm1_encoder_cache(0, 0); dma_register(channel, 0x28).put(control | 1) };
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_dma_aligned_transfer(destination: u32, source: u32, blocks: u32, callback: u32) -> i32 {
    if destination & 31 != 0 || destination == 0 || source & 31 != 0 || blocks == 0 || blocks > 0x7ffffff { return -30; }
    let mut status = unsafe { xm1_pipeline_image_dma_open(0, 0x00918c24 as *mut u32) };
    if status == 0 {
        let program = DmaProgram { control: 0x00455400, register34: 0, register30: 0, source, destination, bytes: blocks.wrapping_shl(5), register0c: core::mem::MaybeUninit::uninit(), register10: core::mem::MaybeUninit::uninit(), register14: core::mem::MaybeUninit::uninit(), link: core::mem::MaybeUninit::uninit(), priority: core::mem::MaybeUninit::uninit() };
        status = unsafe { xm1_pipeline_image_dma_program(Address(0x00918c24).word(), &program) };
        if status == 0 {
            unsafe { Address(0x00918c28).put(callback) };
            let completion = DmaCompletion { argument: 0, callback: xm1_pipeline_image_dma_aligned_completed as *const () as u32 };
            status = unsafe { xm1_pipeline_image_dma_begin(Address(0x00918c24).word(), &completion) };
        }
    }
    if status != 0 { unsafe { xm1_pipeline_image_dma_close(Address(0x00918c24).word()) }; }
    status
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_dma_aligned_completed() {
    let callback = unsafe { Address(0x00918c28).word() };
    if callback != 0 {
        let callback: unsafe extern "C" fn() = unsafe { core::mem::transmute(callback) };
        unsafe { callback() };
    }
    unsafe { Address(0x00918c28).put(0) };
    let handle = unsafe { Address(0x00918c24).word() };
    if handle != 0 { unsafe { Address(0x00918c24).put(0); xm1_pipeline_image_dma_close(handle) }; }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_stream_dma_completed() {
    unsafe { xm1_pipeline_image_send_notification(Address(0x014ddb54).word(), 2) };
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_stream_copy_blocks(destination: u32, source: u32, size: u32) {
    unsafe { xm1_pipeline_image_signal_current_zero(); xm1_pipeline_image_dma_aligned_transfer(destination, source, size.wrapping_add(31) >> 5, xm1_pipeline_image_stream_dma_completed as *const () as u32) };
    let mut events = 0;
    unsafe { xm1_pipeline_image_wait_all(2, &mut events, -1) };
}

unsafe extern "C" {
    fn xm1_pipeline_image_write_template(destination: u32, template: u32) -> u32;
}

#[derive(Clone, Copy)]
enum TemplateElement { Bytes, Halfwords, Words, Ascii, Unknown, End }
impl TemplateElement {
    fn parse(value: u32) -> Self {
        match value { 0 => Self::Bytes, 1 => Self::Halfwords, 2 => Self::Words, 3 => Self::Ascii, u32::MAX => Self::End, _ => Self::Unknown }
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_measure_template(template: u32) -> i32 {
    if template == 0 { return 0; }
    let mut template = Address(template);
    let mut size = 0u32;
    loop {
        match TemplateElement::parse(unsafe { template.word() }) {
            TemplateElement::Bytes => size = size.wrapping_add(unsafe { template.at(4).word() }),
            TemplateElement::Halfwords => size = size.wrapping_add(unsafe { template.at(4).word() }.wrapping_mul(2)),
            TemplateElement::Words => size = size.wrapping_add(unsafe { template.at(4).word() }.wrapping_mul(4)),
            TemplateElement::Ascii => {
                let source = Address(unsafe { template.at(12).word() });
                let mut length = 0u32;
                while unsafe { source.at(length).byte() } != 0 { length = length.wrapping_add(1); }
                size = size.wrapping_add(length.wrapping_add(1).wrapping_mul(2));
            }
            TemplateElement::Unknown => {}
            TemplateElement::End => return size as i32,
        }
        template = template.at(16);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_write_chunked_segments(image: u32) -> u32 {
    let image = Address(image);
    let start = unsafe { image.at(0x420).word() };
    let source = unsafe { image.at(0x3a0).word() };
    let directory = 0x011d4d4c;
    let model = 0x011d4bdc;
    let name = 0x011d4b50;
    let dimensions = 0x011d4c8c;
    let chunk = 0x011d4f10;
    let prefix = unsafe { xm1_pipeline_image_measure_template(directory) } as u32;
    let model_length = unsafe { xm1_pipeline_image_measure_template(model) } as u32;
    let mut cursor = start.wrapping_add(prefix).wrapping_add(model_length);
    let first = unsafe { xm1_pipeline_image_measure_template(name) } as u16;
    let second = unsafe { xm1_pipeline_image_measure_template(dimensions) } as u16;
    unsafe {
        Address(0x014ddb70).put_half(first.wrapping_add(second).wrapping_sub(2));
        Address(0x014ddb68).put(0);
        Address(0x014ddb60).put(image.at(0x394).word());
        Address(0x014ddb64).put(image.at(0x398).word());
    }
    cursor = cursor.wrapping_add(unsafe { xm1_pipeline_image_write_template(cursor, name) });
    cursor = cursor.wrapping_add(unsafe { xm1_pipeline_image_write_template(cursor, dimensions) });
    let mut remaining = unsafe { image.at(0x3ac).word() };
    let mut capacity = 0xffc0u32.wrapping_sub(unsafe { xm1_pipeline_image_measure_template(chunk) } as u32);
    let header = unsafe { xm1_pipeline_image_measure_template(chunk) } as u16;
    unsafe { Address(0x014ddb70).put_half(header.wrapping_sub(2).wrapping_add(capacity as u16)) };
    let chunks = (remaining.wrapping_sub(1).wrapping_add(capacity) / capacity) as u16;
    unsafe { Address(0x014ddb68).put(0); Address(0x014ddb54).put(xm1_pipeline_image_current_task()) };
    let mut padding = 0u16;
    while remaining != 0 {
        if remaining <= capacity {
            let length = remaining.wrapping_sub(2).wrapping_add(unsafe { xm1_pipeline_image_measure_template(chunk) } as u32) as u16;
            let remainder = cursor.wrapping_sub(start).wrapping_add(2).wrapping_add(u32::from(length)) & 63;
            let gap = if remainder == 0 { 0 } else { (64 - remainder) as u16 };
            padding = gap.wrapping_add(2);
            unsafe { Address(0x014ddb70).put_half(padding.wrapping_add(length)) };
            capacity = remaining;
        }
        cursor = cursor.wrapping_add(unsafe { xm1_pipeline_image_write_template(cursor, chunk) });
        let copied = if padding == 0 { capacity } else { capacity.wrapping_sub(2).wrapping_add(u32::from(padding)) };
        unsafe { xm1_pipeline_image_stream_copy_blocks(cursor, source.wrapping_add(Address(0x014ddb68).word()), copied) };
        cursor = cursor.wrapping_add(capacity);
        unsafe { Address(0x014ddb68).put(capacity.wrapping_add(Address(0x014ddb68).word())) };
        remaining = remaining.wrapping_sub(capacity);
    }
    if padding != 0 { unsafe { clear_bytes(Address(cursor), u32::from(padding)) }; }
    let output_start = unsafe { image.at(0x420).word() };
    let total = cursor.wrapping_sub(output_start).wrapping_add(u32::from(padding));
    let first = unsafe { xm1_pipeline_image_measure_template(directory) } as u16;
    let second = unsafe { xm1_pipeline_image_measure_template(model) } as u16;
    unsafe {
        Address(0x014ddb70).put_half(first.wrapping_add(second).wrapping_sub(2));
        Address(0x014ddb6c).put(xm1_pipeline_image_measure_template(dimensions) as u32);
        Address(0x014ddb5c).put(u32::from(chunks).wrapping_mul(47).wrapping_add(image.at(0x3ac).word()).wrapping_add(u32::from(padding)));
    }
    let output = unsafe { image.at(0x420).word() };
    let header = unsafe { xm1_pipeline_image_write_template(output, directory) };
    unsafe { xm1_pipeline_image_write_template(output.wrapping_add(header), model) };
    total
}

