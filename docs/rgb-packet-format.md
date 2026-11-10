# RGB packet format

How the Kinect v2 color stream turns 16 KB USB bulk transfers into one JPEG frame,
and how [`src/rgb/parser.rs`](../src/rgb/parser.rs) takes it apart.

Names below are the ones used in `parser.rs`. The C++ reference is
`libfreenect2/src/rgb_packet_stream_parser.cpp`; where its names differ, see
[Name map](#name-map-rust--c).

## 1. One frame on the wire

```
0        8                              ?       ?               len−56      len
┌────────┬──────────────────────────────┬───────┬───────────────┬───────────┐
│ header │ JPEG  FF D8 … FF D9          │  pad  │    filler     │  footer   │
│  8 B   │ tens of KB                   │ 0–3 B │ filler_length │   56 B    │
└────────┴──────────────────────────────┴───────┴───────────────┴───────────┘
         ▲                                      ▲               ▲           ▲
         RGB_HEADER_SIZE           end of pad / │               │           packet_size
                                 start of filler┘               len − RGB_FOOTER_SIZE
```

| Region  | Size              | Contents                                         |
|---------|-------------------|--------------------------------------------------|
| header  | `RGB_HEADER_SIZE` = 8  | `RawRgbPacketHeader`                        |
| JPEG    | variable          | starts `FF D8`, ends with EOI marker `FF D9`     |
| pad     | 0–3 bytes         | `0xA5` repeated (alignment)                      |
| filler  | `filler_length`   | `"ZZZZ…"` (`0x5A` repeated)                      |
| footer  | `RGB_FOOTER_SIZE` = 56 | `RawRgbPacketFooter`                        |

`footer.packet_size` is the length of the whole thing, header through footer.

All multi-byte fields are **little-endian**.

## 2. Header: `RawRgbPacketHeader`

```
 0          4          8
 ┌──────────┬──────────┬───────────────
 │ sequence │  "BBBB"  │ FF D8 FF E0 …   ← JPEG data starts here
 └──────────┴──────────┴───────────────
              magic_jpeg_header == MAGIC_JPEG (0x42424242)
```

| Offset | Field               | Type | Check                       |
|-------:|---------------------|------|-----------------------------|
| +0     | `sequence`          | u32  | must equal `footer.sequence` |
| +4     | `magic_jpeg_header` | u32  | `== MAGIC_JPEG` (`"BBBB"`)   |

The JPEG is simply `&data[RGB_HEADER_SIZE..]` of the first chunk. The C++ uses a
zero-length array member `jpeg_buffer[0]` to name that same spot; in Rust it's
just a sub-slice.

## 3. Footer: `RawRgbPacketFooter`

The last 56 bytes of a frame: fourteen 4-byte words.
`RawRgbPacketFooter::from_bytes(data)` takes `data.last_chunk::<RGB_FOOTER_SIZE>()`
and hands it to `from_chunk`, which splits it with `as_chunks::<4>()` and
decodes each word with `from_le_bytes`.

| Word | Offset | Field                | Type | Used for                                  |
|-----:|-------:|----------------------|------|-------------------------------------------|
| 0    | +0     | `magic_footer_start` | u32  | `== MAGIC_FOOTER_START` (`0x39393939`, `"9999"`) |
| 1    | +4     | `sequence`           | u32  | `== packet.sequence`                      |
| 2    | +8     | `filler_length`      | u32  | size of the `Z` run before the footer     |
| 3    | +12    | `unknown1`           | u32  | always 0                                  |
| 4    | +16    | `unknown2`           | u32  | always 0                                  |
| 5    | +20    | `timestamp`          | u32  | → `JpegPacket.timestamp`                  |
| 6    | +24    | `exposure`           | f32  | → `JpegPacket.exposure` (~0.5 – 60)       |
| 7    | +28    | `gain`               | f32  | → `JpegPacket.gain` (1.0 – 1.5)           |
| 8    | +32    | `magic_jpeg`         | u32  | `== MAGIC_JPEG` (`0x42424242`, `"BBBB"`)  |
| 9    | +36    | `packet_size`        | u32  | `== RGB_HEADER_SIZE + packet.jpeg_data.len() + data.len()` |
| 10   | +40    | `gamma`              | f32  | → `JpegPacket.gamma` (1.0 – 6.4)          |
| 11–13| +44    | `unknown4[3]`        | u32  | always 0                                  |

`from_bytes` returns `Some(footer)` only when **both** magics match. Each magic
is four identical ASCII bytes, so the check doesn't depend on byte order.

The footer's position depends on the frame length, so it usually isn't 4-byte
aligned. C++ needs `__attribute__((packed))` for that; decoding with
`from_le_bytes` on byte slices never assumes alignment, so Rust doesn't.

## 4. Accumulating a frame

USB transfers are `rgb_xfer_size = 0x4000` (16 KB) on Linux
(`libfreenect2.cpp:847`; 1 MB on Windows, overridable with
`LIBFREENECT2_RGB_TRANSFER_SIZE`). One frame spans many transfers.
`RgbStreamParser::parse_packets(data)` is called once per transfer.

```
             packet.jpeg_data                                  data (this call)
             ───────────────────────────────────────────────►  ─────────────────
call 1   [H|████████]                                           header? start packet
call 2   [H|████████|████████]                                  no footer → append
call 3   [H|████████|████████|████████]                         no footer → append
  …
call N   [H|████████|████████|████████| … |███▒▒░░░░▓▓▓▓]        footer! → get_jpeg_packet
                                              │ │  │   └ footer (56 B)
                                              │ │  └ filler
                                              │ └ pad
                                              └ jpeg_tail
```

What `parse_packets` does:

- **No packet in progress** (`self.packet` is `None`): read `sequence` and
  `magic_jpeg` at offsets 0 and 4 with `read_u32_from_offset`. If
  `magic_jpeg == MAGIC_JPEG`, start a new `JpegPacket` with `jpeg_data`
  pre-allocated to `BUFFER_SIZE` (2 MB) and holding `data[RGB_HEADER_SIZE..]`.
  Otherwise ignore the chunk.
- **Packet in progress**: `self.packet.take()` moves the packet out, then
  `RawRgbPacketFooter::from_bytes(data)` checks the last `RGB_FOOTER_SIZE`
  bytes of *this chunk*.
  - No footer: if `jpeg_data.len() + data.len() > BUFFER_SIZE`, drop the packet
    ("buffer overflow!"); otherwise append `data` and put the packet back with
    `self.packet.replace(packet)`.
  - Footer found: `get_jpeg_packet(packet, data, footer)` validates and
    finishes the frame (sections 5–6). `self.packet` stays `None` either way,
    so the next chunk must start with a new header.

This differs from the C++, which appends every chunk to one buffer `fb` and
checks the tail of the **whole buffer** after each append. Here only the
newest chunk is checked, so the footer itself must sit entirely inside the
last transfer. The filler and pad don't have to (next section).

## 5. Trimming the filler

`get_jpeg_packet` first works out how much of the final chunk is JPEG. Two
cases, depending on whether the filler + footer fit inside `data`:

**Filler starts inside `data`** (`filler_length + RGB_FOOTER_SIZE < data.len()`):

```
data:  [ jpeg_tail … FF D9 A5 A5 | Z Z Z … Z | footer ]
                                 ▲
                                 filler_start = data.len() − RGB_FOOTER_SIZE − filler_length

jpeg_tail = &data[..filler_start]   → appended to packet.jpeg_data
```

The append is checked against `BUFFER_SIZE` first ("buffer overflow!").

**Filler overruns into the previous chunk** (`filler_length + RGB_FOOTER_SIZE >= data.len()`):

```
packet.jpeg_data:  [ … FF D9 A5 | Z Z Z ]      data: [ Z Z … Z | footer ]
                                 └───────┘
                              prev_filler_bytes = filler_length + RGB_FOOTER_SIZE − data.len()

packet.jpeg_data.truncate(len − prev_filler_bytes)   (saturating_sub, so never below 0)
```

Nothing from `data` is appended in this case. Either way, `packet.jpeg_data`
now ends with the JPEG followed by 0–3 pad bytes.

## 6. Finding `FF D9` behind the padding

The pad is 0–3 bytes, so the JPEG ends at one of four positions. For each
`i in 0..4`, `idx = packet.jpeg_data.len().saturating_sub(i)`, and if
`idx >= 2` the two bytes before `idx` are tested:

```
offset from len:     −6  −5  −4  −3  −2  −1 │
packet.jpeg_data: …  3C  11  FF  D9  A5  A5 │ (end)

i = 0  idx = len      bytes [−2, −1] = A5 A5   ✗
i = 1  idx = len − 1  bytes [−3, −2] = D9 A5   ✗
i = 2  idx = len − 2  bytes [−4, −3] = FF D9   ✓  eoi = idx
i = 3  idx = len − 3  bytes [−5, −4] = 11 FF   ✗
```

A miss just means "try the next `i`". The loop doesn't break on a match, so if
two positions matched, the later one (smaller `idx`) would win, same as the C++.
`eoi` stays 0 if none match, and the frame is dropped.

On a match, `packet.jpeg_data.truncate(eoi)` removes the padding.

C++ equivalent (lines 130–148): `length_no_filler` is the JPEG + pad length,
which here is `packet.jpeg_data.len()` after section 5; `eoi` and `jpeg_length`
are both `eoi`; the `idx >= 2` check is line 135's `if (length_no_filler < i + 2) break;`.

## 7. Validation, in order

`get_jpeg_packet` returns `Option<JpegPacket>`. Any failed check logs
"dropped a packet" and returns `None`.

1. `footer.sequence == packet.sequence`.
2. `packet_size == footer.packet_size`, where
   `packet_size = RGB_HEADER_SIZE + packet.jpeg_data.len() + data.len()`.
3. Trim the filler (section 5). Appending `jpeg_tail` must not exceed
   `BUFFER_SIZE`.
4. `FF D9` found within the last 0–3 positions (section 6).
5. Valid: truncate to `eoi`, set `jpeg_data_length`, copy `timestamp`,
   `exposure`, `gain`, `gamma` from the footer, return `Some(packet)`.

## Name map (Rust ↔ C++)

| `parser.rs`                 | `rgb_packet_stream_parser.cpp`          |
|-----------------------------|-----------------------------------------|
| `RgbStreamParser`           | `RgbPacketStreamParser`                 |
| `parse_packets`             | `onDataReceived` (line 82)              |
| `get_jpeg_packet`           | body of the footer `if` (lines 112–174) |
| `JpegPacket`                | `RgbPacket` (`include/internal/libfreenect2/rgb_packet_processor.h:43`) |
| `JpegPacket.jpeg_data`      | `fb.data` (minus the header) / `rgb_packet.jpeg_buffer` |
| `JpegPacket.jpeg_data_length` | `rgb_packet.jpeg_buffer_length`       |
| `RawRgbPacketHeader`        | `RawRgbPacket` (lines 39–44)            |
| `magic_jpeg_header`         | `RawRgbPacket::magic_header`            |
| `&data[RGB_HEADER_SIZE..]`  | `raw_packet->jpeg_buffer`               |
| `RawRgbPacketFooter`        | `RgbPacketFooter` (lines 49–62)         |
| `RawRgbPacketFooter::from_bytes` | `reinterpret_cast` + magic check (lines 110–112) |
| `magic_footer_start` / `MAGIC_FOOTER_START` | `magic_header` / `0x39393939` |
| `magic_jpeg` / `MAGIC_JPEG` | `magic_footer` / `0x42424242`           |
| `RGB_HEADER_SIZE`           | `sizeof(RawRgbPacket)`                  |
| `RGB_FOOTER_SIZE`           | `sizeof(RgbPacketFooter)`               |
| `BUFFER_SIZE`               | `buffer_size_` (line 65)                |
| `packet.jpeg_data.len()` after trimming filler | `length_no_filler` (line 132) |
| `idx`                       | `eoi` (line 137)                        |
| `eoi`                       | `jpeg_length` (line 130)                |
