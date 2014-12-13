// Copyright 2012-2014 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.
//
// ignore-lexer-test FIXME #15883

//! An implementation of SipHash 2-4.
//!
//! See: http://131002.net/siphash/
//!
//! Consider this as a main "general-purpose" hash for all hashtables: it
//! runs at good speed (competitive with spooky and city) and permits
//! strong _keyed_ hashing. Key your hashtables from a strong RNG,
//! such as `rand::Rng`.
//!
//! Although the SipHash algorithm is considered to be cryptographically
//! strong, this implementation has not been reviewed for such purposes.
//! As such, all cryptographic uses of this implementation are strongly
//! discouraged.

use prelude::*;

use default::Default;

use super::{Hash, Hasher, Writer};

/// `SipState` computes a SipHash 2-4 hash over a stream of bytes.
pub struct SipState {
    k0: u64,
    k1: u64,
    length: uint, // how many bytes we've processed
    v0: u64,      // hash state
    v1: u64,
    v2: u64,
    v3: u64,
    tail: u64, // unprocessed bytes le
    ntail: uint,  // how many bytes in tail are valid
}

impl Copy for SipState {}

// sadly, these macro definitions can't appear later,
// because they're needed in the following defs;
// this design could be improved.

macro_rules! u8to64_le (
    ($buf:expr, $i:expr) =>
    ($buf[0+$i] as u64 |
     $buf[1+$i] as u64 << 8 |
     $buf[2+$i] as u64 << 16 |
     $buf[3+$i] as u64 << 24 |
     $buf[4+$i] as u64 << 32 |
     $buf[5+$i] as u64 << 40 |
     $buf[6+$i] as u64 << 48 |
     $buf[7+$i] as u64 << 56);
    ($buf:expr, $i:expr, $len:expr) =>
    ({
        let mut t = 0;
        let mut out = 0u64;
        while t < $len {
            out |= $buf[t+$i] as u64 << t*8;
            t += 1;
        }
        out
    });
)

macro_rules! rotl (
    ($x:expr, $b:expr) =>
    (($x << $b) | ($x >> (64 - $b)))
)

macro_rules! compress (
    ($v0:expr, $v1:expr, $v2:expr, $v3:expr) =>
    ({
        $v0 += $v1; $v1 = rotl!($v1, 13); $v1 ^= $v0;
        $v0 = rotl!($v0, 32);
        $v2 += $v3; $v3 = rotl!($v3, 16); $v3 ^= $v2;
        $v0 += $v3; $v3 = rotl!($v3, 21); $v3 ^= $v0;
        $v2 += $v1; $v1 = rotl!($v1, 17); $v1 ^= $v2;
        $v2 = rotl!($v2, 32);
    })
)

impl SipState {
    /// Creates a `SipState` that is keyed off the provided keys.
    #[inline]
    pub fn new() -> SipState {
        SipState::new_with_keys(0, 0)
    }

    /// Creates a `SipState` that is keyed off the provided keys.
    #[inline]
    pub fn new_with_keys(key0: u64, key1: u64) -> SipState {
        let mut state = SipState {
            k0: key0,
            k1: key1,
            length: 0,
            v0: 0,
            v1: 0,
            v2: 0,
            v3: 0,
            tail: 0,
            ntail: 0,
        };
        state.reset();
        state
    }

    /// Resets the state to its initial state.
    #[inline]
    pub fn reset(&mut self) {
        self.length = 0;
        self.v0 = self.k0 ^ 0x736f6d6570736575;
        self.v1 = self.k1 ^ 0x646f72616e646f6d;
        self.v2 = self.k0 ^ 0x6c7967656e657261;
        self.v3 = self.k1 ^ 0x7465646279746573;
        self.ntail = 0;
    }

    /// Returns the computed hash.
    #[inline]
    pub fn result(&self) -> u64 {
        let mut v0 = self.v0;
        let mut v1 = self.v1;
        let mut v2 = self.v2;
        let mut v3 = self.v3;

        let b: u64 = ((self.length as u64 & 0xff) << 56) | self.tail;

        v3 ^= b;
        compress!(v0, v1, v2, v3);
        compress!(v0, v1, v2, v3);
        v0 ^= b;

        v2 ^= 0xff;
        compress!(v0, v1, v2, v3);
        compress!(v0, v1, v2, v3);
        compress!(v0, v1, v2, v3);
        compress!(v0, v1, v2, v3);

        v0 ^ v1 ^ v2 ^ v3
    }
}

impl Writer for SipState {
    #[inline]
    fn write(&mut self, msg: &[u8]) {
        let length = msg.len();
        self.length += length;

        let mut needed = 0u;

        if self.ntail != 0 {
            needed = 8 - self.ntail;
            if length < needed {
                self.tail |= u8to64_le!(msg, 0, length) << 8*self.ntail;
                self.ntail += length;
                return
            }

            let m = self.tail | u8to64_le!(msg, 0, needed) << 8*self.ntail;

            self.v3 ^= m;
            compress!(self.v0, self.v1, self.v2, self.v3);
            compress!(self.v0, self.v1, self.v2, self.v3);
            self.v0 ^= m;

            self.ntail = 0;
        }

        // Buffered tail is now flushed, process new input.
        let len = length - needed;
        let end = len & (!0x7);
        let left = len & 0x7;

        let mut i = needed;
        while i < end {
            let mi = u8to64_le!(msg, i);

            self.v3 ^= mi;
            compress!(self.v0, self.v1, self.v2, self.v3);
            compress!(self.v0, self.v1, self.v2, self.v3);
            self.v0 ^= mi;

            i += 8;
        }

        self.tail = u8to64_le!(msg, i, left);
        self.ntail = left;
    }
}

impl Clone for SipState {
    #[inline]
    fn clone(&self) -> SipState {
        *self
    }
}

impl Default for SipState {
    #[inline]
    fn default() -> SipState {
        SipState::new()
    }
}

/// `SipHasher` computes the SipHash algorithm from a stream of bytes.
#[deriving(Clone)]
#[allow(missing_copy_implementations)]
pub struct SipHasher {
    k0: u64,
    k1: u64,
}

impl SipHasher {
    /// Creates a `Sip`.
    #[inline]
    pub fn new() -> SipHasher {
        SipHasher::new_with_keys(0, 0)
    }

    /// Creates a `Sip` that is keyed off the provided keys.
    #[inline]
    pub fn new_with_keys(key0: u64, key1: u64) -> SipHasher {
        SipHasher {
            k0: key0,
            k1: key1,
        }
    }
}

impl Hasher<SipState> for SipHasher {
    #[inline]
    fn hash<Sized? T: Hash<SipState>>(&self, value: &T) -> u64 {
        let mut state = SipState::new_with_keys(self.k0, self.k1);
        value.hash(&mut state);
        state.result()
    }
}

impl Default for SipHasher {
    #[inline]
    fn default() -> SipHasher {
        SipHasher::new()
    }
}

/// Hashes a value using the SipHash algorithm.
#[inline]
pub fn hash<Sized? T: Hash<SipState>>(value: &T) -> u64 {
    let mut state = SipState::new();
    value.hash(&mut state);
    state.result()
}

/// Hashes a value with the SipHash algorithm with the provided keys.
#[inline]
pub fn hash_with_keys<Sized? T: Hash<SipState>>(k0: u64, k1: u64, value: &T) -> u64 {
    let mut state = SipState::new_with_keys(k0, k1);
    value.hash(&mut state);
    state.result()
}
