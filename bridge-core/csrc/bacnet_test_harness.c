/**
 * @file bacnet_test_harness.c
 * @brief Thin C wrapper around bacnet-stack encode/decode functions.
 *
 * Compiled for the HOST target (not ARM) by bridge-core's build.rs.
 * Called from Rust #[cfg(test)] via FFI to cross-validate our Rust
 * APDU implementation against the reference bacnet-stack library.
 */

#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include "bacnet/bacdef.h"
#include "bacnet/bacdcode.h"
#include "bacnet/whois.h"
#include "bacnet/iam.h"
#include "bacnet/rp.h"
#include "bacnet/wp.h"

/* ---- Who-Is ---- */

int bacnet_test_encode_whois(
    uint8_t *buf, size_t buf_len,
    int32_t low_limit, int32_t high_limit)
{
    (void)buf_len;
    return whois_encode_apdu(buf, low_limit, high_limit);
}

int bacnet_test_decode_whois(
    const uint8_t *buf, size_t len,
    int32_t *low_limit, int32_t *high_limit)
{
    /* Who-Is APDU starts after the 2-byte header (pdu-type + service) */
    return whois_decode_service_request(buf, (unsigned)len, low_limit, high_limit);
}

/* ---- I-Am ---- */

int bacnet_test_encode_iam(
    uint8_t *buf, size_t buf_len,
    uint32_t device_id,
    uint32_t max_apdu,
    uint8_t segmentation,
    uint16_t vendor_id)
{
    (void)buf_len;
    return iam_encode_apdu(buf, device_id, max_apdu,
                           (BACNET_SEGMENTATION)segmentation, vendor_id);
}

/* ---- ReadProperty ---- */

int bacnet_test_encode_read_property(
    uint8_t *buf, size_t buf_len,
    uint8_t invoke_id,
    uint16_t object_type,
    uint32_t object_instance,
    uint32_t property_id,
    int32_t array_index)
{
    BACNET_READ_PROPERTY_DATA data;
    (void)buf_len;
    data.object_type = (BACNET_OBJECT_TYPE)object_type;
    data.object_instance = object_instance;
    data.object_property = (BACNET_PROPERTY_ID)property_id;
    data.array_index = array_index;
    return rp_encode_apdu(buf, invoke_id, &data);
}

/* ---- WriteProperty ---- */

int bacnet_test_encode_write_property_real(
    uint8_t *buf, size_t buf_len,
    uint8_t invoke_id,
    uint16_t object_type,
    uint32_t object_instance,
    uint32_t property_id,
    float value,
    uint8_t priority)
{
    BACNET_WRITE_PROPERTY_DATA data;
    int apdu_len;
    (void)buf_len;

    memset(&data, 0, sizeof(data));
    data.object_type = (BACNET_OBJECT_TYPE)object_type;
    data.object_instance = object_instance;
    data.object_property = (BACNET_PROPERTY_ID)property_id;
    data.array_index = BACNET_ARRAY_ALL;
    data.priority = priority;

    /* Encode the value into data.application_data */
    apdu_len = encode_application_real(data.application_data, value);
    data.application_data_len = apdu_len;

    return wp_encode_apdu(buf, invoke_id, &data);
}
