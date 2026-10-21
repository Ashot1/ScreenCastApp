#include "TestKeychainSupport.h"
/* Test-only private path, never login/System: SecKeychainCreate does not add it
 * to the search list. Keep deprecation diagnostics visible for these fixture APIs. */
OSStatus ambient_test_keychain_create(const char *path, const void *password, uint32_t length, SecKeychainRef *output) {
    return SecKeychainCreate(path, length, password, false, NULL, output);
}
OSStatus ambient_test_keychain_delete(SecKeychainRef keychain) {
    return SecKeychainDelete(keychain);
}
