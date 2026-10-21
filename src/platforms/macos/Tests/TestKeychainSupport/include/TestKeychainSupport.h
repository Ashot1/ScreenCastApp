#include <Security/Security.h>
OSStatus ambient_test_keychain_create(const char *path, const void *password, uint32_t length, SecKeychainRef *output);
OSStatus ambient_test_keychain_delete(SecKeychainRef keychain);
