#ifndef AMBIENT_VOICE_PROCESS_H
#define AMBIENT_VOICE_PROCESS_H
#include <stddef.h>
#include <stdint.h>
typedef struct ambient_voice_child ambient_voice_child;
ambient_voice_child *ambient_voice_child_spawn(const char *executable, const char *temporary_directory,
                                               const uint8_t *request, size_t count);
int32_t ambient_voice_child_signal(ambient_voice_child *child, int32_t signal_number);
int32_t ambient_voice_child_wait(ambient_voice_child *child);
void ambient_voice_child_destroy(ambient_voice_child *child);
int32_t ambient_voice_read_request(uint8_t *bytes, size_t capacity);
int32_t ambient_voice_watch_parent(void);
#endif
