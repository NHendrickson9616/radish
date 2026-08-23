use std::error::Error;
use std::fs::File as FsFile;
use std::io::ErrorKind;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{
    CODEC_TYPE_AAC, CODEC_TYPE_ALAC, CODEC_TYPE_FLAC, CODEC_TYPE_MP3, CODEC_TYPE_NULL,
    CODEC_TYPE_OPUS, CODEC_TYPE_PCM_F32BE, CODEC_TYPE_PCM_F32LE, CODEC_TYPE_PCM_F64BE,
    CODEC_TYPE_PCM_F64LE, CODEC_TYPE_PCM_S8, CODEC_TYPE_PCM_S16BE, CODEC_TYPE_PCM_S16LE,
    CODEC_TYPE_PCM_S24BE, CODEC_TYPE_PCM_S24LE, CODEC_TYPE_PCM_S32BE, CODEC_TYPE_PCM_S32LE,
    CODEC_TYPE_PCM_U8, CODEC_TYPE_PCM_U16BE, CODEC_TYPE_PCM_U16LE, CODEC_TYPE_PCM_U24BE,
    CODEC_TYPE_PCM_U24LE, CODEC_TYPE_PCM_U32BE, CODEC_TYPE_PCM_U32LE, CODEC_TYPE_VORBIS, CodecType,
    DecoderOptions,
};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::data_model::{AudioCodec, AudioHash};
use crate::operation_model::{
    FieldName, FieldValue, Fields, FutureOperationProducer, Operation, OperationError,
    OperationResult, WorkflowId,
};

pub struct AnalyzeAudio;

impl Operation for AnalyzeAudio {
    fn requires(&self) -> Vec<FieldName> {
        vec!["radish.file".into()]
    }

    fn produces(&self) -> Vec<FieldName> {
        vec!["radish.file".into()]
    }

    fn run(
        &self,
        _workflow_id: WorkflowId,
        inputs: Vec<Fields>,
        _future_operations: &FutureOperationProducer,
    ) -> OperationResult<Fields> {
        let input = inputs
            .first()
            .ok_or_else(|| OperationError::Failed("audio analysis received no input".into()))?;
        let mut file = match input.get("radish.file") {
            Some(FieldValue::File(file)) => file.clone(),
            Some(_) => {
                return Err(
                    OperationError::Failed("`radish.file` has the wrong type".into()).into(),
                );
            }
            None => return Err(OperationError::MissingField("radish.file".into()).into()),
        };

        let (audio_hash, codec, duration_millis) = analyze_audio(&file.path)?;
        file.audio_hash = Some(audio_hash);
        file.codec = Some(codec);
        file.duration_millis = duration_millis;

        let mut output = Fields::default();
        output.insert("radish.file", FieldValue::File(file));
        Ok(output)
    }
}

fn analyze_audio(
    path: &Path,
) -> Result<(AudioHash, AudioCodec, u64), Box<dyn Error + Send + Sync>> {
    let source = Box::new(FsFile::open(path)?);
    let stream = MediaSourceStream::new(source, Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
        hint.with_extension(extension);
    }

    let probed = symphonia::default::get_probe().format(
        &hint,
        stream,
        &FormatOptions {
            enable_gapless: true,
            ..FormatOptions::default()
        },
        &MetadataOptions::default(),
    )?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| std::io::Error::new(ErrorKind::InvalidData, "audio has no default track"))?;
    let track_id = track.id;
    let codec_params = track.codec_params.clone();
    let codec_type = codec_params.codec;
    if codec_type == CODEC_TYPE_OPUS {
        let (audio_hash, duration_millis) =
            crate::opus::hash_opus(&mut *format, track_id, &codec_params)?;
        return Ok((audio_hash, AudioCodec::Opus, duration_millis));
    }
    let mut decoder =
        symphonia::default::get_codecs().make(&codec_params, &DecoderOptions::default())?;

    let mut hasher = PcmHasher::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => {
                break;
            }
            Err(error) => return Err(error.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }

        let decoded = decoder.decode(&packet)?;
        let spec = *decoded.spec();
        let mut samples = SampleBuffer::<i32>::new(decoded.capacity() as u64, spec);
        samples.copy_interleaved_ref(decoded);
        hasher.write(spec.rate, spec.channels.count(), samples.samples())?;
    }

    let duration_millis = hasher.duration_millis()?;
    Ok((
        hasher.finish()?,
        codec_from_type(codec_type),
        duration_millis,
    ))
}

pub(crate) struct PcmHasher {
    hasher: blake3::Hasher,
    stream_spec: Option<(u32, usize)>,
    frame_count: u64,
}

impl PcmHasher {
    pub(crate) fn new() -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"radish.pcm-blake3.v1\0");
        Self {
            hasher,
            stream_spec: None,
            frame_count: 0,
        }
    }

    pub(crate) fn write(
        &mut self,
        sample_rate: u32,
        channels: usize,
        samples: &[i32],
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        if channels == 0 || !samples.len().is_multiple_of(channels) {
            return Err(std::io::Error::new(
                ErrorKind::InvalidData,
                "decoded sample count does not match the channel count",
            )
            .into());
        }

        let current_spec = (sample_rate, channels);
        match self.stream_spec {
            None => {
                self.hasher.update(&sample_rate.to_le_bytes());
                self.hasher.update(&(channels as u32).to_le_bytes());
                self.stream_spec = Some(current_spec);
            }
            Some(previous) if previous != current_spec => {
                return Err(std::io::Error::new(
                    ErrorKind::InvalidData,
                    "audio stream parameters changed while decoding",
                )
                .into());
            }
            Some(_) => {}
        }
        let frames = u64::try_from(samples.len() / channels)?;
        self.frame_count = self.frame_count.checked_add(frames).ok_or_else(|| {
            std::io::Error::new(ErrorKind::InvalidData, "decoded frame count overflow")
        })?;
        for sample in samples {
            self.hasher.update(&sample.to_le_bytes());
        }
        Ok(())
    }

    pub(crate) fn duration_millis(&self) -> Result<u64, Box<dyn Error + Send + Sync>> {
        let (sample_rate, _) = self.stream_spec.ok_or_else(|| {
            std::io::Error::new(ErrorKind::InvalidData, "audio contains no samples")
        })?;
        let millis = u128::from(self.frame_count) * 1_000 / u128::from(sample_rate);
        Ok(u64::try_from(millis)?)
    }

    pub(crate) fn finish(self) -> Result<AudioHash, Box<dyn Error + Send + Sync>> {
        if self.stream_spec.is_none() {
            return Err(
                std::io::Error::new(ErrorKind::InvalidData, "audio contains no samples").into(),
            );
        }
        Ok(AudioHash(format!(
            "pcm-blake3-v1:{}",
            self.hasher.finalize().to_hex()
        )))
    }
}

fn codec_from_type(codec: CodecType) -> AudioCodec {
    match codec {
        CODEC_TYPE_FLAC => AudioCodec::Flac,
        CODEC_TYPE_MP3 => AudioCodec::Mp3,
        CODEC_TYPE_VORBIS => AudioCodec::Vorbis,
        CODEC_TYPE_AAC => AudioCodec::Aac,
        CODEC_TYPE_ALAC => AudioCodec::Alac,
        CODEC_TYPE_OPUS => AudioCodec::Opus,
        CODEC_TYPE_NULL => AudioCodec::Unknown,
        CODEC_TYPE_PCM_S32LE | CODEC_TYPE_PCM_S32BE | CODEC_TYPE_PCM_S24LE
        | CODEC_TYPE_PCM_S24BE | CODEC_TYPE_PCM_S16LE | CODEC_TYPE_PCM_S16BE
        | CODEC_TYPE_PCM_S8 | CODEC_TYPE_PCM_U32LE | CODEC_TYPE_PCM_U32BE
        | CODEC_TYPE_PCM_U24LE | CODEC_TYPE_PCM_U24BE | CODEC_TYPE_PCM_U16LE
        | CODEC_TYPE_PCM_U16BE | CODEC_TYPE_PCM_U8 | CODEC_TYPE_PCM_F32LE
        | CODEC_TYPE_PCM_F32BE | CODEC_TYPE_PCM_F64LE | CODEC_TYPE_PCM_F64BE => AudioCodec::Pcm,
        _ => AudioCodec::Other(format!("{codec:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FILE_ID: AtomicU64 = AtomicU64::new(1);

    fn write_test_wav(samples: &[i16]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "radish-audio-hash-{}-{}.wav",
            std::process::id(),
            NEXT_FILE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let data_size = (samples.len() * 2) as u32;
        let mut wav = Vec::with_capacity(44 + data_size as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_size).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&8_000_u32.to_le_bytes());
        wav.extend_from_slice(&16_000_u32.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_size.to_le_bytes());
        for sample in samples {
            wav.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(&path, wav).unwrap();
        path
    }

    #[test]
    fn decoded_audio_hash_is_stable_and_extracts_codec() {
        let first_path = write_test_wav(&[0, 1_000, -1_000, i16::MAX]);
        let second_path = write_test_wav(&[0, 1_000, -1_000, i16::MAX]);
        let different_path = write_test_wav(&[0, 1_000, -999, i16::MAX]);

        let (first_hash, codec, _) = analyze_audio(&first_path).unwrap();
        let (second_hash, _, _) = analyze_audio(&second_path).unwrap();
        let (different_hash, _, _) = analyze_audio(&different_path).unwrap();

        assert_eq!(codec, AudioCodec::Pcm);
        assert_eq!(first_hash, second_hash);
        assert_ne!(first_hash, different_hash);
        assert!(first_hash.0.starts_with("pcm-blake3-v1:"));

        std::fs::remove_file(first_path).unwrap();
        std::fs::remove_file(second_path).unwrap();
        std::fs::remove_file(different_path).unwrap();
    }

    #[test]
    fn duration_is_calculated_from_decoded_frames() {
        let path = write_test_wav(&vec![0; 8_000]);

        let (_, _, duration_millis) = analyze_audio(&path).unwrap();

        assert_eq!(duration_millis, 1_000);
        std::fs::remove_file(path).unwrap();
    }
}
