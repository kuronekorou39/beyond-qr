//! beyond-qr-core
//!
//! 光学データ伝送のコアロジック。色パレット、色空間変換、誤り訂正符号、
//! フレームレイアウト、エンコーダ、デコーダを提供する。
//!
//! Phase 0a: メモリ上で bytes → Frame → bytes の往復が完全一致することを保証する。

pub mod color_space;
pub mod decoder;
pub mod ecc;
pub mod encoder;
pub mod frame;
pub mod palette;

pub use decoder::{decode_payload, DecodeError};
pub use encoder::{encode_payload, EncodeError};
pub use frame::{Frame, FrameSpec};
pub use palette::{Color, Rgb};
