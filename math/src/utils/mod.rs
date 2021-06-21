// Copyright (c) Facebook, Inc. and its affiliates.
//
// This source code is licensed under the MIT license found in the
// LICENSE file in the root directory of this source tree.

use crate::{errors::SerializationError, field::FieldElement};
use utils::{batch_iter_mut, iter_mut, uninit_vector};

#[cfg(feature = "concurrent")]
use rayon::prelude::*;

#[cfg(test)]
mod tests;

// MATH FUNCTIONS
// ================================================================================================

/// Generates a vector with values [1, b, b^2, b^3, b^4, ..., b^(n-1)].
/// When `concurrent` feature is enabled, series generation is done concurrently in multiple
/// threads.
pub fn get_power_series<E: FieldElement>(b: E, n: usize) -> Vec<E> {
    let mut result = unsafe { uninit_vector(n) };
    batch_iter_mut!(&mut result, 1024, |batch: &mut [E], batch_offset: usize| {
        let start = b.exp((batch_offset as u64).into());
        fill_power_series(batch, b, start);
    });
    result
}

/// Generates a vector with values [s, s * b, s * b^2, s * b^3, s * b^4, ..., s * b^(n-1)].
/// When `concurrent` feature is enabled, series generation is done concurrently in multiple
/// threads.
pub fn get_power_series_with_offset<E: FieldElement>(b: E, s: E, n: usize) -> Vec<E> {
    let mut result = unsafe { uninit_vector(n) };
    batch_iter_mut!(&mut result, 1024, |batch: &mut [E], batch_offset: usize| {
        let start = s * b.exp((batch_offset as u64).into());
        fill_power_series(batch, b, start);
    });
    result
}

/// Computes a[i] + b[i] for all i and stores the results in a.
pub fn add_in_place<E: FieldElement>(a: &mut [E], b: &[E]) {
    assert!(
        a.len() == b.len(),
        "number of values must be the same for both operands"
    );
    iter_mut!(a).zip(b).for_each(|(a, &b)| *a += b);
}

/// Computes a[i] + b[i] * c for all i and saves result into a.
pub fn mul_acc<B, E>(a: &mut [E], b: &[B], c: E)
where
    B: FieldElement,
    E: FieldElement + From<B>,
{
    assert!(
        a.len() == b.len(),
        "number of values must be the same for both slices"
    );
    iter_mut!(a).zip(b).for_each(|(a, &b)| *a += E::from(b) * c);
}

/// Computes a multiplicative inverse of a sequence of elements using batch inversion method.
/// Any ZEROs in the provided sequence are ignored.
pub fn batch_inversion<E: FieldElement>(values: &[E]) -> Vec<E> {
    let mut result: Vec<E> = unsafe { uninit_vector(values.len()) };
    batch_iter_mut!(&mut result, 1024, |batch: &mut [E], batch_offset: usize| {
        let start = batch_offset;
        let end = start + batch.len();
        serial_batch_inversion(&values[start..end], batch);
    });
    result
}

/// Returns base 2 logarithm of `n`, where `n` is a power of two.
pub fn log2(n: usize) -> u32 {
    assert!(n.is_power_of_two(), "n must be a power of two");
    n.trailing_zeros()
}

// HELPER FUNCTIONS
// ------------------------------------------------------------------------------------------------

#[inline(always)]
fn fill_power_series<E: FieldElement>(result: &mut [E], base: E, start: E) {
    result[0] = start;
    for i in 1..result.len() {
        result[i] = result[i - 1] * base;
    }
}

fn serial_batch_inversion<E: FieldElement>(values: &[E], result: &mut [E]) {
    let mut last = E::ONE;
    for (result, &value) in result.iter_mut().zip(values.iter()) {
        *result = last;
        if value != E::ZERO {
            last *= value;
        }
    }

    last = last.inv();

    for i in (0..values.len()).rev() {
        if values[i] == E::ZERO {
            result[i] = E::ZERO;
        } else {
            result[i] *= last;
            last *= values[i];
        }
    }
}

// VECTOR FUNCTIONS
// ================================================================================================

/// Returns a vector with zero elements removed from the end of the vector.
pub fn remove_leading_zeros<E: FieldElement>(values: &[E]) -> Vec<E> {
    for i in (0..values.len()).rev() {
        if values[i] != E::ZERO {
            return values[..(i + 1)].to_vec();
        }
    }

    [].to_vec()
}

// SERIALIZATION / DESERIALIZATION
// ================================================================================================

/// Reads elements from the specified `source` and copies them into the provided `destination`
/// The elements are assumed to be stored in the slice one after the other in little-endian
/// byte order. When no errors are encountered, returns the number of read elements.
///
/// Returns an error if:
/// * Number of bytes in the `source` does not divide evenly into whole number of elements.
/// * Size of the destination slice is not sufficient to hold all elements read from the source.
/// * Underlying `source` bytes do not represent a sequence of valid field elements.
pub fn read_elements_into<E: FieldElement>(
    source: &[u8],
    destination: &mut [E],
) -> Result<usize, SerializationError> {
    if source.len() % E::ELEMENT_BYTES != 0 {
        return Err(SerializationError::NotEnoughBytesForWholeElements(
            source.len(),
        ));
    }
    let num_elements = source.len() / E::ELEMENT_BYTES;
    if destination.len() < num_elements {
        return Err(SerializationError::DestinationTooSmall(
            num_elements,
            destination.len(),
        ));
    }

    for i in (0..source.len()).step_by(E::ELEMENT_BYTES) {
        match E::try_from(&source[i..i + E::ELEMENT_BYTES]) {
            Ok(value) => destination[i / E::ELEMENT_BYTES] = value,
            Err(_) => return Err(SerializationError::FailedToReadElement(i)),
        }
    }

    Ok(num_elements)
}

/// Returns a vector of elements read from the provided slice of bytes. The elements are
/// assumed to be stored in the slice one after the other in little-endian byte order.
///
/// Returns an error if:
/// * Number of bytes in the `source` does not divide evenly into whole number of elements.
/// * Underlying `source` bytes do not represent a sequence of valid field elements.
pub fn read_elements_into_vec<E: FieldElement>(
    source: &[u8],
) -> Result<Vec<E>, SerializationError> {
    if source.len() % E::ELEMENT_BYTES != 0 {
        return Err(SerializationError::NotEnoughBytesForWholeElements(
            source.len(),
        ));
    }

    let num_elements = source.len() / E::ELEMENT_BYTES;
    let mut result = unsafe { uninit_vector(num_elements) };
    read_elements_into(source, &mut result)?;
    Ok(result)
}
