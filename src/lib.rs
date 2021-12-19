use std::io::Result;

use ffmpeg_next::{
  codec,
  color::Range,
  encoder::{self, video::Video},
  format::{
    context::{output, Output},
    output, Flags, Pixel,
  },
  frame,
  sys::av_rescale_q,
  Dictionary, Packet, Rational,
};
use jni::{objects::JString, JNIEnv};

const PIXEL_FORMAT: Pixel = Pixel::YUV444P;
const OPTS: [(&str, &str); 3] = [
  ("preset", "ultrafast"),
  ("profile", "high444"),
  ("crf", "13"),
];

struct JavaFrame {
  av_frame: frame::Video,
  original_yuv: (*mut u8, *mut u8, *mut u8),
}

unsafe impl Sync for JavaFrame {}

impl JavaFrame {
  fn new(
    width: u32,
    height: u32,
    jvm_y_channel: *mut u8,
    jvm_u_channel: *mut u8,
    jvm_v_channel: *mut u8,
  ) -> JavaFrame {
    let mut av_frame = frame::Video::new(PIXEL_FORMAT, width, height);
    av_frame.set_color_range(Range::JPEG);

    // Store the original yuv buffers for later cleanup
    let original_yuv = unsafe {
      (
        (*av_frame.as_mut_ptr()).data[0],
        (*av_frame.as_mut_ptr()).data[1],
        (*av_frame.as_mut_ptr()).data[2],
      )
    };

    // Change the underlying buffer that's in use by these frames
    unsafe {
      (*av_frame.as_mut_ptr()).data[0] = jvm_y_channel;
      (*av_frame.as_mut_ptr()).data[1] = jvm_u_channel;
      (*av_frame.as_mut_ptr()).data[2] = jvm_v_channel;
    }

    JavaFrame {
      av_frame,
      original_yuv,
    }
  }
}

struct Renderer {
  frame_a: JavaFrame,
  frame_b: JavaFrame,
  frame_index: u64,
  frame_rate: Rational,
  encoder: Video,
  octx: Output,
  stream_time_base: Rational,
}

impl Renderer {
  fn new(
    output_file: String,
    width: u32,
    height: u32,
    frame_rate: Rational,
    frame_a: JavaFrame,
    frame_b: JavaFrame,
  ) -> Result<Renderer> {
    let mut octx = output(&output_file)?;
    let global_header = octx.format().flags().contains(Flags::GLOBAL_HEADER);
    let mut ost = octx.add_stream(encoder::find_by_name("libx264"))?;
    let mut encoder = ost.codec().encoder().video()?;
    // encoder.set_bit_rate(20000000);
    // TODO - set crf value here ahhahheahh
    encoder.set_width(width);
    encoder.set_height(height);
    encoder.set_format(PIXEL_FORMAT);
    encoder.set_color_range(Range::JPEG);
    encoder.set_frame_rate(Some(frame_rate));
    encoder.set_time_base(frame_rate.invert());
    if global_header {
      encoder.set_flags(codec::Flags::GLOBAL_HEADER);
    }

    encoder.open_with(Dictionary::from_iter(OPTS))?;

    encoder = ost.codec().encoder().video()?;
    ost.set_parameters(encoder);

    unsafe {
      // TODO - ???
      // (*ost.parameters().as_mut_ptr()).codec_tag = 0;
    }

    let encoder = ost.codec().encoder().video()?;

    output::dump(&octx, 0, Some(&output_file));
    octx.write_header()?;
    let stream_time_base = octx.stream(0).map_or(Rational(90000, 1), |s| s.time_base());

    Ok(Renderer {
      frame_a,
      frame_b,
      frame_index: 0,
      frame_rate,
      encoder,
      octx,
      stream_time_base,
    })
  }

  fn send_frame(&mut self, use_buffer_b: bool) {
    let pts = unsafe {
      av_rescale_q(
        self.frame_index as _,
        self.frame_rate.invert().into(),
        self.stream_time_base.into(),
      )
    };

    let frame = if use_buffer_b {
      &mut self.frame_b
    } else {
      &mut self.frame_a
    };

    println!("oh I see buffer b? {}", use_buffer_b);
    frame.av_frame.set_pts(Some(pts as i64));

    println!("About to send_frame {}", self.frame_index);
    self.encoder.send_frame(&frame.av_frame).unwrap();

    println!("Sent frame, receiving packet {}", self.frame_index);
    let mut encoded = Packet::empty();
    while self.encoder.receive_packet(&mut encoded).is_ok() {
      println!("Received packet, writing {}", self.frame_index);
      encoded.set_stream(0);
      println!("actually writing {}", self.frame_index);
      // TODO - ^^^ do we need this when we're like doing audio and stuff?

      encoded.write_interleaved(&mut self.octx).unwrap();
    }
    println!("wrote or didnt sent the frame haha {}", self.frame_index);

    self.frame_index += 1;
  }

  fn finish_render(&mut self) -> Result<()> {
    self.encoder.send_eof()?;
    let mut encoded = Packet::empty();
    while self.encoder.receive_packet(&mut encoded).is_ok() {
      encoded.write_interleaved(&mut self.octx).unwrap();
    }
    self.octx.write_trailer()?;

    Ok(())
  }
}

static mut RENDERER_STATE: Option<Renderer> = None;

#[no_mangle]
extern "C" fn Java_me_aris_recordingmod_RendererKt_startEncode(
  env: JNIEnv,
  _: *const (),
  file: JString,
  width: u32,
  height: u32,
  fps: i32,
  y_a: *mut u8,
  u_a: *mut u8,
  v_a: *mut u8,
  y_b: *mut u8,
  u_b: *mut u8,
  v_b: *mut u8,
) {
  let frame_a = JavaFrame::new(width, height, y_a, u_a, v_a);
  let frame_b = JavaFrame::new(width, height, y_b, u_b, v_b);
  unsafe {
    RENDERER_STATE = Some(
      Renderer::new(
        env.get_string(file).unwrap().into(),
        width,
        height,
        Rational(fps, 1),
        frame_a,
        frame_b,
      )
      .unwrap(),
    );
  }
}

#[no_mangle]
extern "C" fn Java_me_aris_recordingmod_RendererKt_sendFrame(
  _: *const (),
  _: *const (),
  use_bufer_b: bool,
) {
  let renderer = unsafe { &mut RENDERER_STATE };
  if let Some(renderer) = renderer {
    renderer.send_frame(use_bufer_b);
  }
}

#[no_mangle]
extern "C" fn Java_me_aris_recordingmod_RendererKt_finishEncode(
  _: *const (),
  _: *const (),
) {
  let renderer = unsafe { &mut RENDERER_STATE };
  if let Some(renderer) = renderer {
    let _ = renderer.finish_render();
  }
  unsafe { RENDERER_STATE = None }
}

impl Drop for JavaFrame {
  fn drop(&mut self) {
    // Revert the underlying data so that it can be destroyed properly
    let (y, u, v) = self.original_yuv;
    unsafe {
      (*self.av_frame.as_mut_ptr()).data[0] = y;
      (*self.av_frame.as_mut_ptr()).data[1] = u;
      (*self.av_frame.as_mut_ptr()).data[2] = v;
    }
  }
}
