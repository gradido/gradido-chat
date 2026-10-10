//! Two soft notes for a new message, made here: no sound file to ship or license.

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn new_message() {
    use rodio::buffer::SamplesBuffer;
    use std::num::NonZero;

    // The device is opened per sound and closed after it: an idle chat
    // holds no audio stream open.
    std::thread::spawn(|| {
        const RATE: u32 = 44_100;
        let mut samples = Vec::new();
        for (frequency, seconds) in [(659.25_f32, 0.12_f32), (880.0, 0.22)] {
            let count = (RATE as f32 * seconds) as usize;
            for i in 0..count {
                let t = i as f32 / RATE as f32;
                // A short fade at both ends, or the note clicks.
                let edge = (i.min(count - i) as f32 / (RATE as f32 * 0.01)).min(1.0);
                samples.push((t * frequency * std::f32::consts::TAU).sin() * 0.18 * edge);
            }
        }
        let Ok(mut sink) = rodio::DeviceSinkBuilder::open_default_sink() else {
            return;
        };
        sink.log_on_drop(false);
        let player = rodio::Player::connect_new(sink.mixer());
        player.append(SamplesBuffer::new(NonZero::new(1).unwrap(), NonZero::new(RATE).unwrap(), samples));
        player.sleep_until_end();
    });
}

// On a phone the notification brings its own sound.
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn new_message() {}
