use crate::types::Uuid;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    BadBool = 1,
    BadUtf8 = 2,
    OptCondition = 3,
    BadEnum = 4,
    BadTag = 5,
    Truncated = 6,
    ChunkInconsistent = 7,
    BadOrder = 8,
    BadShape = 9,
}

impl Reason {
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            1 => Self::BadBool,
            2 => Self::BadUtf8,
            3 => Self::OptCondition,
            4 => Self::BadEnum,
            5 => Self::BadTag,
            6 => Self::Truncated,
            7 => Self::ChunkInconsistent,
            8 => Self::BadOrder,
            9 => Self::BadShape,
            _ => return None,
        })
    }
}

pub type DResult<T> = Result<T, Reason>;

pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    pub fn take(&mut self, n: usize) -> DResult<&'a [u8]> {
        if self.remaining() < n {
            return Err(Reason::Truncated);
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    pub fn array<const N: usize>(&mut self) -> DResult<[u8; N]> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    pub fn u8(&mut self) -> DResult<u8> {
        Ok(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> DResult<u16> {
        Ok(u16::from_be_bytes(self.array()?))
    }

    pub fn u32(&mut self) -> DResult<u32> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    pub fn u64(&mut self) -> DResult<u64> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    pub fn i32(&mut self) -> DResult<i32> {
        Ok(i32::from_be_bytes(self.array()?))
    }

    pub fn bool(&mut self) -> DResult<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Reason::BadBool),
        }
    }

    pub fn len_prefixed(&mut self) -> DResult<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }

    pub fn string(&mut self) -> DResult<String> {
        let raw = self.len_prefixed()?;
        std::str::from_utf8(raw)
            .map(str::to_owned)
            .map_err(|_| Reason::BadUtf8)
    }

    pub fn uuid(&mut self) -> DResult<Uuid> {
        Ok(Uuid(self.array()?))
    }

    pub fn opt<T>(&mut self, f: impl FnOnce(&mut Self) -> DResult<T>) -> DResult<Option<T>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(f(self)?)),
            _ => Err(Reason::BadTag),
        }
    }

    pub fn list<T>(&mut self, mut f: impl FnMut(&mut Self) -> DResult<T>) -> DResult<Vec<T>> {
        let n = self.u32()? as usize;
        let mut v = Vec::with_capacity(n.min(self.remaining()));
        for _ in 0..n {
            v.push(f(self)?);
        }
        Ok(v)
    }
}

pub trait Put {
    fn put_u8(&mut self, v: u8);
    fn put_u16(&mut self, v: u16);
    fn put_u32(&mut self, v: u32);
    fn put_u64(&mut self, v: u64);
    fn put_i32(&mut self, v: i32);
    fn put_bool(&mut self, v: bool);
    fn put_raw(&mut self, v: &[u8]);
    fn put_len_prefixed(&mut self, v: &[u8]);
}

impl Put for Vec<u8> {
    fn put_u8(&mut self, v: u8) {
        self.push(v);
    }
    fn put_u16(&mut self, v: u16) {
        self.extend_from_slice(&v.to_be_bytes());
    }
    fn put_u32(&mut self, v: u32) {
        self.extend_from_slice(&v.to_be_bytes());
    }
    fn put_u64(&mut self, v: u64) {
        self.extend_from_slice(&v.to_be_bytes());
    }
    fn put_i32(&mut self, v: i32) {
        self.extend_from_slice(&v.to_be_bytes());
    }
    fn put_bool(&mut self, v: bool) {
        self.push(v as u8);
    }
    fn put_raw(&mut self, v: &[u8]) {
        self.extend_from_slice(v);
    }
    fn put_len_prefixed(&mut self, v: &[u8]) {
        self.put_u32(u32::try_from(v.len()).expect("FXSH length field exceeds u32"));
        self.extend_from_slice(v);
    }
}

pub trait Codec: Sized {
    fn enc(&self, w: &mut Vec<u8>);
    fn dec(r: &mut Reader<'_>) -> DResult<Self>;
}

macro_rules! prim {
    ($t:ty, $put:ident, $get:ident) => {
        impl Codec for $t {
            fn enc(&self, w: &mut Vec<u8>) {
                w.$put(*self);
            }
            fn dec(r: &mut Reader<'_>) -> DResult<Self> {
                r.$get()
            }
        }
    };
}

prim!(u8, put_u8, u8);
prim!(u16, put_u16, u16);
prim!(u32, put_u32, u32);
prim!(u64, put_u64, u64);
prim!(i32, put_i32, i32);
prim!(bool, put_bool, bool);

impl Codec for String {
    fn enc(&self, w: &mut Vec<u8>) {
        w.put_len_prefixed(self.as_bytes());
    }
    fn dec(r: &mut Reader<'_>) -> DResult<Self> {
        r.string()
    }
}

impl Codec for Uuid {
    fn enc(&self, w: &mut Vec<u8>) {
        w.put_raw(&self.0);
    }
    fn dec(r: &mut Reader<'_>) -> DResult<Self> {
        r.uuid()
    }
}

impl<T: Codec> Codec for Option<T> {
    fn enc(&self, w: &mut Vec<u8>) {
        match self {
            None => w.put_u8(0),
            Some(v) => {
                w.put_u8(1);
                v.enc(w);
            }
        }
    }
    fn dec(r: &mut Reader<'_>) -> DResult<Self> {
        r.opt(T::dec)
    }
}

impl<T: Codec> Codec for Vec<T> {
    fn enc(&self, w: &mut Vec<u8>) {
        w.put_u32(u32::try_from(self.len()).expect("FXSH list count exceeds u32"));
        for v in self {
            v.enc(w);
        }
    }
    fn dec(r: &mut Reader<'_>) -> DResult<Self> {
        r.list(T::dec)
    }
}

impl<A: Codec, B: Codec> Codec for (A, B) {
    fn enc(&self, w: &mut Vec<u8>) {
        self.0.enc(w);
        self.1.enc(w);
    }
    fn dec(r: &mut Reader<'_>) -> DResult<Self> {
        Ok((A::dec(r)?, B::dec(r)?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Bytes(pub Vec<u8>);

impl Codec for Bytes {
    fn enc(&self, w: &mut Vec<u8>) {
        w.put_len_prefixed(&self.0);
    }
    fn dec(r: &mut Reader<'_>) -> DResult<Self> {
        Ok(Bytes(r.len_prefixed()?.to_vec()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Digest(pub [u8; 16]);

impl Codec for Digest {
    fn enc(&self, w: &mut Vec<u8>) {
        w.put_raw(&self.0);
    }
    fn dec(r: &mut Reader<'_>) -> DResult<Self> {
        Ok(Digest(r.array()?))
    }
}

macro_rules! record {
    ($(#[$m:meta])* $name:ident { $($f:ident : $t:ty),* $(,)? }) => {
        record!($(#[$m])* $name { $($f: $t),* } check |_s| Ok(()));
    };
    ($(#[$m:meta])* $name:ident { $($f:ident : $t:ty),* $(,)? } check $chk:expr) => {
        $(#[$m])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $name { $(pub $f: $t),* }
        impl $crate::wire::Codec for $name {
            #[allow(unused_variables)]
            fn enc(&self, w: &mut Vec<u8>) {
                $( $crate::wire::Codec::enc(&self.$f, w); )*
            }
            #[allow(unused_variables)]
            fn dec(r: &mut $crate::wire::Reader<'_>) -> $crate::wire::DResult<Self> {
                let v = Self { $($f: $crate::wire::Codec::dec(r)?),* };
                let check: fn(&$name) -> $crate::wire::DResult<()> = $chk;
                check(&v)?;
                Ok(v)
            }
        }
    };
}

macro_rules! u8_enum {
    ($(#[$m:meta])* $name:ident { $($v:ident = $n:literal),* $(,)? }) => {
        $(#[$m])*
        #[repr(u8)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name { $($v = $n),* }
        impl $name {
            pub fn from_u8(v: u8) -> Option<Self> {
                match v { $($n => Some(Self::$v),)* _ => None }
            }
        }
        impl $crate::wire::Codec for $name {
            fn enc(&self, w: &mut Vec<u8>) {
                $crate::wire::Put::put_u8(w, *self as u8);
            }
            fn dec(r: &mut $crate::wire::Reader<'_>) -> $crate::wire::DResult<Self> {
                Self::from_u8(r.u8()?).ok_or($crate::wire::Reason::BadEnum)
            }
        }
    };
}

pub(crate) use record;
pub(crate) use u8_enum;

impl Codec for Reason {
    fn enc(&self, w: &mut Vec<u8>) {
        w.put_u8(*self as u8);
    }
    fn dec(r: &mut Reader<'_>) -> DResult<Self> {
        Reason::from_u8(r.u8()?).ok_or(Reason::BadEnum)
    }
}

pub fn strictly_ascending<T, K: PartialOrd>(items: &[T], key: impl Fn(&T) -> K) -> DResult<()> {
    for pair in items.windows(2) {
        if key(&pair[0]) >= key(&pair[1]) {
            return Err(Reason::BadOrder);
        }
    }
    Ok(())
}
