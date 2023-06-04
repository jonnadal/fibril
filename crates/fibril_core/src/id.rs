use core::fmt::{Debug, Display, Formatter};

#[cfg(feature = "std")]
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    ops::{Index, IndexMut},
};

#[derive(Clone, Copy, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Id(usize);

impl Debug for Id {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), core::fmt::Error> {
        Display::fmt(self, f)
    }
}

impl Display for Id {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        if self.0 < 256 * 256 {
            f.write_str(":")?;
            Display::fmt(&self.0, f)
        } else {
            #[cfg(feature = "std")]
            {
                Display::fmt(&SocketAddrV4::from(*self), f)
            }
            #[cfg(not(feature = "std"))]
            {
                Debug::fmt(&self, f)
            }
        }
    }
}

#[cfg(feature = "std")]
impl From<SocketAddr> for Id {
    fn from(addr: SocketAddr) -> Self {
        match addr {
            SocketAddr::V4(v4) => v4.into(),
            SocketAddr::V6(_) => unimplemented!(),
        }
    }
}

#[cfg(feature = "std")]
impl From<SocketAddrV4> for Id {
    fn from(addr: SocketAddrV4) -> Self {
        let octets = addr.ip().octets();
        let port_bytes = addr.port().to_be_bytes();
        let mut result: [u8; 8] = [0; 8];
        result[0] = 0;
        result[1] = 0;
        result[2] = octets[0];
        result[3] = octets[1];
        result[4] = octets[2];
        result[5] = octets[3];
        result[6] = port_bytes[0];
        result[7] = port_bytes[1];
        Id(usize::from_be_bytes(result))
    }
}

impl From<Id> for usize {
    fn from(id: Id) -> Self {
        id.0
    }
}

impl From<usize> for Id {
    fn from(n: usize) -> Self {
        Id(n)
    }
}

#[cfg(feature = "std")]
impl From<Id> for SocketAddrV4 {
    fn from(id: Id) -> Self {
        let bytes = id.0.to_be_bytes();
        let ip = Ipv4Addr::from([bytes[2], bytes[3], bytes[4], bytes[5]]);
        let port = u16::from_be_bytes([bytes[6], bytes[7]]);
        SocketAddrV4::new(ip, port)
    }
}

impl<T> Index<Id> for [T] {
    type Output = T;
    fn index(&self, id: Id) -> &Self::Output {
        self.index(usize::from(id))
    }
}

impl<T> IndexMut<Id> for [T] {
    fn index_mut(&mut self, id: Id) -> &mut Self::Output {
        self.index_mut(usize::from(id))
    }
}

impl<T> Index<Id> for Vec<T> {
    type Output = T;
    fn index(&self, id: Id) -> &Self::Output {
        self.index(usize::from(id))
    }
}

impl<T> IndexMut<Id> for Vec<T> {
    fn index_mut(&mut self, id: Id) -> &mut Self::Output {
        self.index_mut(usize::from(id))
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Id {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        SocketAddrV4::deserialize(deserializer).map(|v4| v4.into())
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Id {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        SocketAddrV4::from(*self).serialize(serializer)
    }
}
