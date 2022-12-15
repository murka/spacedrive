//! This module contains the crate's STREAM implementation, and wrappers that allow us to support multiple AEADs.
use std::io::{Cursor, Read, Write};

use crate::{
	primitives::{AEAD_TAG_SIZE, BLOCK_SIZE, KEY_LEN},
	Error, Protected, Result,
};
use aead::{
	stream::{DecryptorLE31, EncryptorLE31},
	KeyInit, Payload,
};
use aes_gcm::Aes256Gcm;
use chacha20poly1305::XChaCha20Poly1305;

/// These are all possible algorithms that can be used for encryption and decryption
#[derive(Clone, Copy, Eq, PartialEq)]
#[cfg_attr(
	feature = "serde",
	derive(serde::Serialize),
	derive(serde::Deserialize)
)]
#[cfg_attr(feature = "rspc", derive(specta::Type))]
#[allow(clippy::use_self)]
pub enum Algorithm {
	XChaCha20Poly1305,
	Aes256Gcm,
}

impl Algorithm {
	/// This function allows us to calculate the nonce length for a given algorithm
	#[must_use]
	pub const fn nonce_len(&self) -> usize {
		match self {
			Self::XChaCha20Poly1305 => 20,
			Self::Aes256Gcm => 8,
		}
	}
}

pub enum StreamEncryption {
	XChaCha20Poly1305(Box<EncryptorLE31<XChaCha20Poly1305>>),
	Aes256Gcm(Box<EncryptorLE31<Aes256Gcm>>),
}

pub enum StreamDecryption {
	Aes256Gcm(Box<DecryptorLE31<Aes256Gcm>>),
	XChaCha20Poly1305(Box<DecryptorLE31<XChaCha20Poly1305>>),
}

impl StreamEncryption {
	/// This should be used to initialize a stream encryption object.
	///
	/// The master key, a suitable nonce, and a specific algorithm should be provided.
	#[allow(clippy::needless_pass_by_value)]
	pub fn new(key: Protected<[u8; KEY_LEN]>, nonce: &[u8], algorithm: Algorithm) -> Result<Self> {
		if nonce.len() != algorithm.nonce_len() {
			return Err(Error::NonceLengthMismatch);
		}

		let encryption_object = match algorithm {
			Algorithm::XChaCha20Poly1305 => {
				let cipher = XChaCha20Poly1305::new_from_slice(key.expose())
					.map_err(|_| Error::StreamModeInit)?;

				let stream = EncryptorLE31::from_aead(cipher, nonce.into());
				Self::XChaCha20Poly1305(Box::new(stream))
			}
			Algorithm::Aes256Gcm => {
				let cipher =
					Aes256Gcm::new_from_slice(key.expose()).map_err(|_| Error::StreamModeInit)?;

				let stream = EncryptorLE31::from_aead(cipher, nonce.into());
				Self::Aes256Gcm(Box::new(stream))
			}
		};

		Ok(encryption_object)
	}

	fn encrypt_next<'msg, 'aad>(
		&mut self,
		payload: impl Into<Payload<'msg, 'aad>>,
	) -> aead::Result<Vec<u8>> {
		match self {
			Self::XChaCha20Poly1305(s) => s.encrypt_next(payload),
			Self::Aes256Gcm(s) => s.encrypt_next(payload),
		}
	}

	fn encrypt_last<'msg, 'aad>(
		self,
		payload: impl Into<Payload<'msg, 'aad>>,
	) -> aead::Result<Vec<u8>> {
		match self {
			Self::XChaCha20Poly1305(s) => s.encrypt_last(payload),
			Self::Aes256Gcm(s) => s.encrypt_last(payload),
		}
	}

	/// This function should be used for encrypting large amounts of data.
	///
	/// The streaming implementation reads blocks of data in `BLOCK_SIZE`, encrypts, and writes to the writer.
	///
	/// It requires a reader, a writer, and any AAD to go with it.
	///
	/// The AAD will be authenticated with each block of data.
	pub fn encrypt_streams<R, W>(mut self, mut reader: R, mut writer: W, aad: &[u8]) -> Result<()>
	where
		R: Read,
		W: Write,
	{
		let mut read_buffer = vec![0u8; BLOCK_SIZE].into_boxed_slice();
		loop {
			let read_count = reader.read(&mut read_buffer)?;
			if read_count == BLOCK_SIZE {
				let payload = Payload {
					aad,
					msg: &read_buffer,
				};

				let encrypted_data = self.encrypt_next(payload).map_err(|_| Error::Encrypt)?;

				writer.write_all(&encrypted_data)?;
			} else {
				// we use `..read_count` in order to only use the read data, and not zeroes also
				let payload = Payload {
					aad,
					msg: &read_buffer[..read_count],
				};

				let encrypted_data = self.encrypt_last(payload).map_err(|_| Error::Encrypt)?;
				writer.write_all(&encrypted_data)?;

				break;
			}
		}

		writer.flush()?;

		Ok(())
	}

	/// This should ideally only be used for small amounts of data
	///
	/// It is just a thin wrapper around `encrypt_streams()`, but reduces the amount of code needed elsewhere.
	#[allow(unused_mut)]
	pub fn encrypt_bytes(
		key: Protected<[u8; KEY_LEN]>,
		nonce: &[u8],
		algorithm: Algorithm,
		bytes: &[u8],
		aad: &[u8],
	) -> Result<Vec<u8>> {
		let mut writer = Cursor::new(Vec::<u8>::new());
		let encryptor = Self::new(key, nonce, algorithm)?;

		encryptor
			.encrypt_streams(bytes, &mut writer, aad)
			.map_or_else(Err, |_| Ok(writer.into_inner()))
	}
}

impl StreamDecryption {
	/// This should be used to initialize a stream decryption object.
	///
	/// The master key, nonce and algorithm that were used for encryption should be provided.
	#[allow(clippy::needless_pass_by_value)]
	pub fn new(key: Protected<[u8; KEY_LEN]>, nonce: &[u8], algorithm: Algorithm) -> Result<Self> {
		if nonce.len() != algorithm.nonce_len() {
			return Err(Error::NonceLengthMismatch);
		}

		let decryption_object = match algorithm {
			Algorithm::XChaCha20Poly1305 => {
				let cipher = XChaCha20Poly1305::new_from_slice(key.expose())
					.map_err(|_| Error::StreamModeInit)?;

				let stream = DecryptorLE31::from_aead(cipher, nonce.into());
				Self::XChaCha20Poly1305(Box::new(stream))
			}
			Algorithm::Aes256Gcm => {
				let cipher =
					Aes256Gcm::new_from_slice(key.expose()).map_err(|_| Error::StreamModeInit)?;

				let stream = DecryptorLE31::from_aead(cipher, nonce.into());
				Self::Aes256Gcm(Box::new(stream))
			}
		};

		Ok(decryption_object)
	}

	fn decrypt_next<'msg, 'aad>(
		&mut self,
		payload: impl Into<Payload<'msg, 'aad>>,
	) -> aead::Result<Vec<u8>> {
		match self {
			Self::XChaCha20Poly1305(s) => s.decrypt_next(payload),
			Self::Aes256Gcm(s) => s.decrypt_next(payload),
		}
	}

	fn decrypt_last<'msg, 'aad>(
		self,
		payload: impl Into<Payload<'msg, 'aad>>,
	) -> aead::Result<Vec<u8>> {
		match self {
			Self::XChaCha20Poly1305(s) => s.decrypt_last(payload),
			Self::Aes256Gcm(s) => s.decrypt_last(payload),
		}
	}

	/// This function should be used for decrypting large amounts of data.
	///
	/// The streaming implementation reads blocks of data in `BLOCK_SIZE`, decrypts, and writes to the writer.
	///
	/// It requires a reader, a writer, and any AAD that was used.
	///
	/// The AAD will be authenticated with each block of data - if the AAD doesn't match what was used during encryption, an error will be returned.
	pub fn decrypt_streams<R, W>(mut self, mut reader: R, mut writer: W, aad: &[u8]) -> Result<()>
	where
		R: Read,
		W: Write,
	{
		let mut read_buffer = vec![0u8; BLOCK_SIZE + AEAD_TAG_SIZE].into_boxed_slice();

		loop {
			let read_count = reader.read(&mut read_buffer)?;
			if read_count == (BLOCK_SIZE + AEAD_TAG_SIZE) {
				let payload = Payload {
					aad,
					msg: &read_buffer,
				};

				let decrypted_data = self.decrypt_next(payload).map_err(|_| Error::Decrypt)?;

				writer.write_all(&decrypted_data)?;
			} else {
				let payload = Payload {
					aad,
					msg: &read_buffer[..read_count],
				};

				let decrypted_data = self.decrypt_last(payload).map_err(|_| Error::Decrypt)?;
				writer.write_all(&decrypted_data)?;

				break;
			}
		}

		writer.flush()?;

		Ok(())
	}

	/// This should ideally only be used for small amounts of data
	///
	/// It is just a thin wrapper around `decrypt_streams()`, but reduces the amount of code needed elsewhere.
	#[allow(unused_mut)]
	pub fn decrypt_bytes(
		key: Protected<[u8; KEY_LEN]>,
		nonce: &[u8],
		algorithm: Algorithm,
		bytes: &[u8],
		aad: &[u8],
	) -> Result<Protected<Vec<u8>>> {
		let mut writer = Cursor::new(Vec::<u8>::new());
		let decryptor = Self::new(key, nonce, algorithm)?;

		decryptor
			.decrypt_streams(bytes, &mut writer, aad)
			.map_or_else(Err, |_| Ok(Protected::new(writer.into_inner())))
	}
}
