use std::error::Error;
use std::io::ErrorKind;

use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatReader;

use crate::audio_hash::PcmHasher;
use crate::data_model::AudioHash;

const OPUS_SAMPLE_RATE: u32 = 48_000;
const MAX_FRAME_SAMPLES: usize = 5_760;

pub(crate) fn hash_opus(
    format: &mut dyn FormatReader,
    track_id: u32,
    codec_params: &CodecParameters,
) -> Result<(AudioHash, u64), Box<dyn Error + Send + Sync>> {
    let header = OpusHeader::parse(
        codec_params
            .extra_data
            .as_deref()
            .ok_or_else(|| invalid_data("Opus stream has no OpusHead data"))?,
    )?;
    let mut decoder = PacketDecoder::new(&header)?;
    let mut hasher = PcmHasher::new();
    let mut output = vec![0_i16; MAX_FRAME_SAMPLES * header.channels];

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

        let decoded_frames = decoder.decode(&packet.data, &mut output)?;
        let trim_start = packet.trim_start as usize;
        let trim_end = packet.trim_end as usize;
        if trim_start + trim_end > decoded_frames {
            return Err(invalid_data("Opus packet trim exceeds decoded frame count").into());
        }

        let start = trim_start * header.channels;
        let end = (decoded_frames - trim_end) * header.channels;
        let normalized = output[start..end]
            .iter()
            .map(|sample| (*sample as i32) << 16)
            .collect::<Vec<_>>();
        hasher.write(OPUS_SAMPLE_RATE, header.channels, &normalized)?;
    }

    let duration_millis = hasher.duration_millis()?;
    Ok((hasher.finish()?, duration_millis))
}

#[derive(Debug)]
struct OpusHeader {
    channels: usize,
    gain_q8: i32,
    mapping: ChannelMapping,
}

#[derive(Debug)]
enum ChannelMapping {
    MonoOrStereo,
    Multistream {
        streams: u8,
        coupled_streams: u8,
        mapping: Vec<u8>,
    },
}

impl OpusHeader {
    fn parse(data: &[u8]) -> Result<Self, Box<dyn Error + Send + Sync>> {
        if data.len() < 19 || &data[..8] != b"OpusHead" {
            return Err(invalid_data("invalid OpusHead data").into());
        }
        let channels = data[9] as usize;
        if channels == 0 {
            return Err(invalid_data("OpusHead declares zero channels").into());
        }
        let gain_q8 = i16::from_le_bytes([data[16], data[17]]) as i32;
        let mapping = match data[18] {
            0 if channels <= 2 => ChannelMapping::MonoOrStereo,
            0 => {
                return Err(invalid_data(
                    "Opus channel mapping family 0 supports only mono or stereo",
                )
                .into());
            }
            1 => {
                let required_length = 21 + channels;
                if data.len() < required_length {
                    return Err(invalid_data("truncated Opus multistream mapping").into());
                }
                ChannelMapping::Multistream {
                    streams: data[19],
                    coupled_streams: data[20],
                    mapping: data[21..required_length].to_vec(),
                }
            }
            family => {
                return Err(invalid_data(format!(
                    "unsupported Opus channel mapping family {family}"
                ))
                .into());
            }
        };
        Ok(Self {
            channels,
            gain_q8,
            mapping,
        })
    }
}

enum PacketDecoder {
    MonoOrStereo(::opus::Decoder),
    Multistream(::opus::MSDecoder),
}

impl PacketDecoder {
    fn new(header: &OpusHeader) -> Result<Self, Box<dyn Error + Send + Sync>> {
        match &header.mapping {
            ChannelMapping::MonoOrStereo => {
                let channels = if header.channels == 1 {
                    ::opus::Channels::Mono
                } else {
                    ::opus::Channels::Stereo
                };
                let mut decoder = ::opus::Decoder::new(OPUS_SAMPLE_RATE, channels)?;
                decoder.set_gain(header.gain_q8)?;
                Ok(Self::MonoOrStereo(decoder))
            }
            ChannelMapping::Multistream {
                streams,
                coupled_streams,
                mapping,
            } => {
                let mut decoder =
                    ::opus::MSDecoder::new(OPUS_SAMPLE_RATE, *streams, *coupled_streams, mapping)?;
                decoder.set_gain(header.gain_q8)?;
                Ok(Self::Multistream(decoder))
            }
        }
    }

    fn decode(
        &mut self,
        packet: &[u8],
        output: &mut [i16],
    ) -> Result<usize, Box<dyn Error + Send + Sync>> {
        let frames = match self {
            Self::MonoOrStereo(decoder) => decoder.decode(packet, output, false)?,
            Self::Multistream(decoder) => decoder.decode(packet, output, false)?,
        };
        Ok(frames)
    }
}

fn invalid_data(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mono_header() -> Vec<u8> {
        let mut header = Vec::from(&b"OpusHead"[..]);
        header.push(1);
        header.push(1);
        header.extend_from_slice(&0_u16.to_le_bytes());
        header.extend_from_slice(&OPUS_SAMPLE_RATE.to_le_bytes());
        header.extend_from_slice(&0_i16.to_le_bytes());
        header.push(0);
        header
    }

    #[test]
    fn decodes_a_raw_mono_opus_packet() {
        let header = OpusHeader::parse(&mono_header()).unwrap();
        let mut encoder = ::opus::Encoder::new(
            OPUS_SAMPLE_RATE,
            ::opus::Channels::Mono,
            ::opus::Application::Audio,
        )
        .unwrap();
        let input = (0..960)
            .map(|sample| ((sample as f32 / 20.0).sin() * 10_000.0) as i16)
            .collect::<Vec<_>>();
        let mut encoded = vec![0_u8; 4_000];
        let encoded_length = encoder.encode(&input, &mut encoded).unwrap();

        let mut decoder = PacketDecoder::new(&header).unwrap();
        let mut output = vec![0_i16; MAX_FRAME_SAMPLES];
        let frames = decoder
            .decode(&encoded[..encoded_length], &mut output)
            .unwrap();

        assert_eq!(frames, input.len());
        assert!(output[..frames].iter().any(|sample| *sample != 0));
    }

    #[test]
    fn rejects_an_unsupported_mapping_family() {
        let mut header = mono_header();
        header[18] = 2;

        let error = OpusHeader::parse(&header).unwrap_err();

        assert!(error.to_string().contains("mapping family 2"));
    }
}
