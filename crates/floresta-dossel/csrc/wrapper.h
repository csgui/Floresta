/* SPDX-License-Identifier: MIT OR Apache-2.0 */

/* The single translation unit bindgen parses: the system Guile headers plus
 * Dossel's shim declarations. See dossel_shim.h for why the shim exists. */

#include <libguile.h>

#include "dossel_shim.h"
