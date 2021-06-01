// Re implementation of WriteBytesExt from byteorder for Async
use std::io::Result;
use byteorder::{ByteOrder};
use futures::prelude::*;
use async_trait::async_trait;

/// Extends [`Write`] with methods for writing numbers. (For `std::io`.)
///
/// Most of the methods defined here have an unconstrained type parameter that
/// must be explicitly instantiated. Typically, it is instantiated with either
/// the [`BigEndian`] or [`LittleEndian`] types defined in this crate.
///
/// # Examples
///
/// Write unsigned 16 bit big-endian integers to a [`Write`]:
///
/// ```rust
/// use byteorder::{BigEndian, WriteBytesExt};
///
/// let mut wtr = vec![];
/// wtr.write_u16::<BigEndian>(517).unwrap();
/// wtr.write_u16::<BigEndian>(768).unwrap();
/// assert_eq!(wtr, vec![2, 5, 3, 0]);
/// ```
///
/// [`BigEndian`]: enum.BigEndian.html
/// [`LittleEndian`]: enum.LittleEndian.html
/// [`Write`]: https://doc.rust-lang.org/std/io/trait.Write.html
#[async_trait]
pub(crate) trait AsyncWriteBytesExt: futures::io::AsyncWrite + Unpin {
    /// Writes an unsigned 8 bit integer to the underlying writer.
    ///
    /// Note that since this writes a single byte, no byte order conversions
    /// are used. It is included for completeness.
    ///
    /// # Errors
    ///
    /// This method returns the same errors as [`Write::write_all`].
    ///
    /// [`Write::write_all`]: https://doc.rust-lang.org/std/io/trait.Write.html#method.write_all
    ///
    /// # Examples
    ///
    /// Write unsigned 8 bit integers to a `Write`:
    ///
    /// ```rust
    /// use byteorder::WriteBytesExt;
    ///
    /// let mut wtr = Vec::new();
    /// wtr.write_u8(2).unwrap();
    /// wtr.write_u8(5).unwrap();
    /// assert_eq!(wtr, b"\x02\x05");
    /// ```
    async fn write_u8(&mut self, n: u8) -> Result<()> {
        self.write_all(&[n]).await
    }

    /// Writes a signed 8 bit integer to the underlying writer.
    ///
    /// Note that since this writes a single byte, no byte order conversions
    /// are used. It is included for completeness.
    ///
    /// # Errors
    ///
    /// This method returns the same errors as [`Write::write_all`].
    ///
    /// [`Write::write_all`]: https://doc.rust-lang.org/std/io/trait.Write.html#method.write_all
    ///
    /// # Examples
    ///
    /// Write signed 8 bit integers to a `Write`:
    ///
    /// ```rust
    /// use byteorder::WriteBytesExt;
    ///
    /// let mut wtr = Vec::new();
    /// wtr.write_i8(2).unwrap();
    /// wtr.write_i8(-5).unwrap();
    /// assert_eq!(wtr, b"\x02\xfb");
    /// ```
    async fn write_i8(&mut self, n: i8) -> Result<()> {
        self.write_all(&[n as u8]).await
    }

    /// Writes an unsigned 16 bit integer to the underlying writer.
    ///
    /// # Errors
    ///
    /// This method returns the same errors as [`Write::write_all`].
    ///
    /// [`Write::write_all`]: https://doc.rust-lang.org/std/io/trait.Write.html#method.write_all
    ///
    /// # Examples
    ///
    /// Write unsigned 16 bit big-endian integers to a `Write`:
    ///
    /// ```rust
    /// use byteorder::{BigEndian, WriteBytesExt};
    ///
    /// let mut wtr = Vec::new();
    /// wtr.write_u16::<BigEndian>(517).unwrap();
    /// wtr.write_u16::<BigEndian>(768).unwrap();
    /// assert_eq!(wtr, b"\x02\x05\x03\x00");
    /// ```
    async fn write_u16<T: ByteOrder>(&mut self, n: u16) -> Result<()> {
        let mut buf = [0; 2];
        T::write_u16(&mut buf, n);
        self.write_all(&buf).await
    }

    /// Writes a signed 16 bit integer to the underlying writer.
    ///
    /// # Errors
    ///
    /// This method returns the same errors as [`Write::write_all`].
    ///
    /// [`Write::write_all`]: https://doc.rust-lang.org/std/io/trait.Write.html#method.write_all
    ///
    /// # Examples
    ///
    /// Write signed 16 bit big-endian integers to a `Write`:
    ///
    /// ```rust
    /// use byteorder::{BigEndian, WriteBytesExt};
    ///
    /// let mut wtr = Vec::new();
    /// wtr.write_i16::<BigEndian>(193).unwrap();
    /// wtr.write_i16::<BigEndian>(-132).unwrap();
    /// assert_eq!(wtr, b"\x00\xc1\xff\x7c");
    /// ```
    async fn write_i16<T: ByteOrder>(&mut self, n: i16) -> Result<()> {
        let mut buf = [0; 2];
        T::write_i16(&mut buf, n);
        self.write_all(&buf).await
    }

    /// Writes an unsigned 24 bit integer to the underlying writer.
    ///
    /// # Errors
    ///
    /// This method returns the same errors as [`Write::write_all`].
    ///
    /// [`Write::write_all`]: https://doc.rust-lang.org/std/io/trait.Write.html#method.write_all
    ///
    /// # Examples
    ///
    /// Write unsigned 24 bit big-endian integers to a `Write`:
    ///
    /// ```rust
    /// use byteorder::{BigEndian, WriteBytesExt};
    ///
    /// let mut wtr = Vec::new();
    /// wtr.write_u24::<BigEndian>(267).unwrap();
    /// wtr.write_u24::<BigEndian>(120111).unwrap();
    /// assert_eq!(wtr, b"\x00\x01\x0b\x01\xd5\x2f");
    /// ```
    async fn write_u24<T: ByteOrder>(&mut self, n: u32) -> Result<()> {
        let mut buf = [0; 3];
        T::write_u24(&mut buf, n);
        self.write_all(&buf).await
    }

    /// Writes a signed 24 bit integer to the underlying writer.
    ///
    /// # Errors
    ///
    /// This method returns the same errors as [`Write::write_all`].
    ///
    /// [`Write::write_all`]: https://doc.rust-lang.org/std/io/trait.Write.html#method.write_all
    ///
    /// # Examples
    ///
    /// Write signed 24 bit big-endian integers to a `Write`:
    ///
    /// ```rust
    /// use byteorder::{BigEndian, WriteBytesExt};
    ///
    /// let mut wtr = Vec::new();
    /// wtr.write_i24::<BigEndian>(-34253).unwrap();
    /// wtr.write_i24::<BigEndian>(120111).unwrap();
    /// assert_eq!(wtr, b"\xff\x7a\x33\x01\xd5\x2f");
    /// ```
    async fn write_i24<T: ByteOrder>(&mut self, n: i32) -> Result<()> {
        let mut buf = [0; 3];
        T::write_i24(&mut buf, n);
        self.write_all(&buf).await
    }

    /// Writes an unsigned 32 bit integer to the underlying writer.
    ///
    /// # Errors
    ///
    /// This method returns the same errors as [`Write::write_all`].
    ///
    /// [`Write::write_all`]: https://doc.rust-lang.org/std/io/trait.Write.html#method.write_all
    ///
    /// # Examples
    ///
    /// Write unsigned 32 bit big-endian integers to a `Write`:
    ///
    /// ```rust
    /// use byteorder::{BigEndian, WriteBytesExt};
    ///
    /// let mut wtr = Vec::new();
    /// wtr.write_u32::<BigEndian>(267).unwrap();
    /// wtr.write_u32::<BigEndian>(1205419366).unwrap();
    /// assert_eq!(wtr, b"\x00\x00\x01\x0b\x47\xd9\x3d\x66");
    /// ```
    async fn write_u32<T: ByteOrder>(&mut self, n: u32) -> Result<()> {
        let mut buf = [0; 4];
        T::write_u32(&mut buf, n);
        self.write_all(&buf).await
    }

    /// Writes a signed 32 bit integer to the underlying writer.
    ///
    /// # Errors
    ///
    /// This method returns the same errors as [`Write::write_all`].
    ///
    /// [`Write::write_all`]: https://doc.rust-lang.org/std/io/trait.Write.html#method.write_all
    ///
    /// # Examples
    ///
    /// Write signed 32 bit big-endian integers to a `Write`:
    ///
    /// ```rust
    /// use byteorder::{BigEndian, WriteBytesExt};
    ///
    /// let mut wtr = Vec::new();
    /// wtr.write_i32::<BigEndian>(-34253).unwrap();
    /// wtr.write_i32::<BigEndian>(1205419366).unwrap();
    /// assert_eq!(wtr, b"\xff\xff\x7a\x33\x47\xd9\x3d\x66");
    /// ```
    async fn write_i32<T: ByteOrder>(&mut self, n: i32) -> Result<()> {
        let mut buf = [0; 4];
        T::write_i32(&mut buf, n);
        self.write_all(&buf).await
    }

    /// Writes an unsigned 48 bit integer to the underlying writer.
    ///
    /// # Errors
    ///
    /// This method returns the same errors as [`Write::write_all`].
    ///
    /// [`Write::write_all`]: https://doc.rust-lang.org/std/io/trait.Write.html#method.write_all
    ///
    /// # Examples
    ///
    /// Write unsigned 48 bit big-endian integers to a `Write`:
    ///
    /// ```rust
    /// use byteorder::{BigEndian, WriteBytesExt};
    ///
    /// let mut wtr = Vec::new();
    /// wtr.write_u48::<BigEndian>(52360336390828).unwrap();
    /// wtr.write_u48::<BigEndian>(541).unwrap();
    /// assert_eq!(wtr, b"\x2f\x9f\x17\x40\x3a\xac\x00\x00\x00\x00\x02\x1d");
    /// ```
    async fn write_u48<T: ByteOrder>(&mut self, n: u64) -> Result<()> {
        let mut buf = [0; 6];
        T::write_u48(&mut buf, n);
        self.write_all(&buf).await
    }

    /// Writes a signed 48 bit integer to the underlying writer.
    ///
    /// # Errors
    ///
    /// This method returns the same errors as [`Write::write_all`].
    ///
    /// [`Write::write_all`]: https://doc.rust-lang.org/std/io/trait.Write.html#method.write_all
    ///
    /// # Examples
    ///
    /// Write signed 48 bit big-endian integers to a `Write`:
    ///
    /// ```rust
    /// use byteorder::{BigEndian, WriteBytesExt};
    ///
    /// let mut wtr = Vec::new();
    /// wtr.write_i48::<BigEndian>(-108363435763825).unwrap();
    /// wtr.write_i48::<BigEndian>(77).unwrap();
    /// assert_eq!(wtr, b"\x9d\x71\xab\xe7\x97\x8f\x00\x00\x00\x00\x00\x4d");
    /// ```
    async fn write_i48<T: ByteOrder>(&mut self, n: i64) -> Result<()> {
        let mut buf = [0; 6];
        T::write_i48(&mut buf, n);
        self.write_all(&buf).await
    }

    /// Writes an unsigned 64 bit integer to the underlying writer.
    ///
    /// # Errors
    ///
    /// This method returns the same errors as [`Write::write_all`].
    ///
    /// [`Write::write_all`]: https://doc.rust-lang.org/std/io/trait.Write.html#method.write_all
    ///
    /// # Examples
    ///
    /// Write unsigned 64 bit big-endian integers to a `Write`:
    ///
    /// ```rust
    /// use byteorder::{BigEndian, WriteBytesExt};
    ///
    /// let mut wtr = Vec::new();
    /// wtr.write_u64::<BigEndian>(918733457491587).unwrap();
    /// wtr.write_u64::<BigEndian>(143).unwrap();
    /// assert_eq!(wtr, b"\x00\x03\x43\x95\x4d\x60\x86\x83\x00\x00\x00\x00\x00\x00\x00\x8f");
    /// ```
    async fn write_u64<T: ByteOrder>(&mut self, n: u64) -> Result<()> {
        let mut buf = [0; 8];
        T::write_u64(&mut buf, n);
        self.write_all(&buf).await
    }

    /// Writes a signed 64 bit integer to the underlying writer.
    ///
    /// # Errors
    ///
    /// This method returns the same errors as [`Write::write_all`].
    ///
    /// [`Write::write_all`]: https://doc.rust-lang.org/std/io/trait.Write.html#method.write_all
    ///
    /// # Examples
    ///
    /// Write signed 64 bit big-endian integers to a `Write`:
    ///
    /// ```rust
    /// use byteorder::{BigEndian, WriteBytesExt};
    ///
    /// let mut wtr = Vec::new();
    /// wtr.write_i64::<BigEndian>(i64::min_value()).unwrap();
    /// wtr.write_i64::<BigEndian>(i64::max_value()).unwrap();
    /// assert_eq!(wtr, b"\x80\x00\x00\x00\x00\x00\x00\x00\x7f\xff\xff\xff\xff\xff\xff\xff");
    /// ```
    async fn write_i64<T: ByteOrder>(&mut self, n: i64) -> Result<()> {
        let mut buf = [0; 8];
        T::write_i64(&mut buf, n);
        self.write_all(&buf).await
    }

    /// Writes an unsigned 128 bit integer to the underlying writer.
    async fn write_u128<T: ByteOrder>(&mut self, n: u128) -> Result<()> {
        let mut buf = [0; 16];
        T::write_u128(&mut buf, n);
        self.write_all(&buf).await
    }

    /// Writes a signed 128 bit integer to the underlying writer.
    async fn write_i128<T: ByteOrder>(&mut self, n: i128) -> Result<()> {
        let mut buf = [0; 16];
        T::write_i128(&mut buf, n);
        self.write_all(&buf).await
    }

    /// Writes an unsigned n-bytes integer to the underlying writer.
    ///
    /// # Errors
    ///
    /// This method returns the same errors as [`Write::write_all`].
    ///
    /// [`Write::write_all`]: https://doc.rust-lang.org/std/io/trait.Write.html#method.write_all
    ///
    /// # Panics
    ///
    /// If the given integer is not representable in the given number of bytes,
    /// this method panics. If `nbytes > 8`, this method panics.
    ///
    /// # Examples
    ///
    /// Write unsigned 40 bit big-endian integers to a `Write`:
    ///
    /// ```rust
    /// use byteorder::{BigEndian, WriteBytesExt};
    ///
    /// let mut wtr = Vec::new();
    /// wtr.write_uint::<BigEndian>(312550384361, 5).unwrap();
    /// wtr.write_uint::<BigEndian>(43, 5).unwrap();
    /// assert_eq!(wtr, b"\x48\xc5\x74\x62\xe9\x00\x00\x00\x00\x2b");
    /// ```
    async fn write_uint<T: ByteOrder>(
        &mut self,
        n: u64,
        nbytes: usize,
    ) -> Result<()> {
        let mut buf = [0; 8];
        T::write_uint(&mut buf, n, nbytes);
        self.write_all(&buf[0..nbytes]).await
    }

    /// Writes a signed n-bytes integer to the underlying writer.
    ///
    /// # Errors
    ///
    /// This method returns the same errors as [`Write::write_all`].
    ///
    /// [`Write::write_all`]: https://doc.rust-lang.org/std/io/trait.Write.html#method.write_all
    ///
    /// # Panics
    ///
    /// If the given integer is not representable in the given number of bytes,
    /// this method panics. If `nbytes > 8`, this method panics.
    ///
    /// # Examples
    ///
    /// Write signed 56 bit big-endian integers to a `Write`:
    ///
    /// ```rust
    /// use byteorder::{BigEndian, WriteBytesExt};
    ///
    /// let mut wtr = Vec::new();
    /// wtr.write_int::<BigEndian>(-3548172039376767, 7).unwrap();
    /// wtr.write_int::<BigEndian>(43, 7).unwrap();
    /// assert_eq!(wtr, b"\xf3\x64\xf4\xd1\xfd\xb0\x81\x00\x00\x00\x00\x00\x00\x2b");
    /// ```
    async fn write_int<T: ByteOrder>(
        &mut self,
        n: i64,
        nbytes: usize,
    ) -> Result<()> {
        let mut buf = [0; 8];
        T::write_int(&mut buf, n, nbytes);
        self.write_all(&buf[0..nbytes]).await
    }

    /// Writes an unsigned n-bytes integer to the underlying writer.
    ///
    /// If the given integer is not representable in the given number of bytes,
    /// this method panics. If `nbytes > 16`, this method panics.
    async fn write_uint128<T: ByteOrder>(
        &mut self,
        n: u128,
        nbytes: usize,
    ) -> Result<()> {
        let mut buf = [0; 16];
        T::write_uint128(&mut buf, n, nbytes);
        self.write_all(&buf[0..nbytes]).await
    }

    /// Writes a signed n-bytes integer to the underlying writer.
    ///
    /// If the given integer is not representable in the given number of bytes,
    /// this method panics. If `nbytes > 16`, this method panics.
    async fn write_int128<T: ByteOrder>(
        &mut self,
        n: i128,
        nbytes: usize,
    ) -> Result<()> {
        let mut buf = [0; 16];
        T::write_int128(&mut buf, n, nbytes);
        self.write_all(&buf[0..nbytes]).await
    }

    /// Writes a IEEE754 single-precision (4 bytes) floating point number to
    /// the underlying writer.
    ///
    /// # Errors
    ///
    /// This method returns the same errors as [`Write::write_all`].
    ///
    /// [`Write::write_all`]: https://doc.rust-lang.org/std/io/trait.Write.html#method.write_all
    ///
    /// # Examples
    ///
    /// Write a big-endian single-precision floating point number to a `Write`:
    ///
    /// ```rust
    /// use std::f32;
    ///
    /// use byteorder::{BigEndian, WriteBytesExt};
    ///
    /// let mut wtr = Vec::new();
    /// wtr.write_f32::<BigEndian>(f32::consts::PI).unwrap();
    /// assert_eq!(wtr, b"\x40\x49\x0f\xdb");
    /// ```
    async fn write_f32<T: ByteOrder>(&mut self, n: f32) -> Result<()> {
        let mut buf = [0; 4];
        T::write_f32(&mut buf, n);
        self.write_all(&buf).await
    }

    /// Writes a IEEE754 double-precision (8 bytes) floating point number to
    /// the underlying writer.
    ///
    /// # Errors
    ///
    /// This method returns the same errors as [`Write::write_all`].
    ///
    /// [`Write::write_all`]: https://doc.rust-lang.org/std/io/trait.Write.html#method.write_all
    ///
    /// # Examples
    ///
    /// Write a big-endian double-precision floating point number to a `Write`:
    ///
    /// ```rust
    /// use std::f64;
    ///
    /// use byteorder::{BigEndian, WriteBytesExt};
    ///
    /// let mut wtr = Vec::new();
    /// wtr.write_f64::<BigEndian>(f64::consts::PI).unwrap();
    /// assert_eq!(wtr, b"\x40\x09\x21\xfb\x54\x44\x2d\x18");
    /// ```
    async fn write_f64<T: ByteOrder>(&mut self, n: f64) -> Result<()> {
        let mut buf = [0; 8];
        T::write_f64(&mut buf, n);
        self.write_all(&buf).await
    }
}

/// All types that implement `Write` get methods defined in `WriteBytesExt`
/// for free.
impl<W: futures::io::AsyncWrite + Unpin + ?Sized> AsyncWriteBytesExt for W {}