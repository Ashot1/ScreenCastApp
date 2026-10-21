#ifndef AMBIENT_LOCAL_PERMISSIONS_H
#define AMBIENT_LOCAL_PERMISSIONS_H

#include <ApplicationServices/ApplicationServices.h>

// The SDK declares this option key as a mutable C global; keep that read at the native boundary.
static inline CFStringRef _Nonnull ambient_accessibility_prompt_key(void) CF_RETURNS_NOT_RETAINED {
    return kAXTrustedCheckOptionPrompt;
}

#endif
