#include <stddef.h>
#include "rc_client.h"
#include "rc_api_request.h"
#include "rc_hash.h"

int rcheevos_sys_hash_buffer(char hash[33], uint32_t console_id, const uint8_t* buffer, size_t buffer_size) {
  rc_hash_iterator_t iterator;
  int ok;
  rc_hash_initialize_iterator(&iterator, "rom", buffer, buffer_size);
  ok = rc_hash_generate(hash, console_id, &iterator);
  rc_hash_destroy_iterator(&iterator);
  return ok;
}

size_t rcheevos_sys_layout(int which) {
  switch (which) {
    case 0: return sizeof(rc_api_server_response_t);
    case 1: return sizeof(rc_client_user_t);
    case 2: return sizeof(rc_client_user_game_summary_t);
    case 3: return sizeof(rc_client_game_t);
    case 4: return sizeof(rc_client_achievement_t);
    case 5: return sizeof(rc_client_achievement_bucket_t);
    case 6: return sizeof(rc_client_achievement_list_t);
    case 7: return sizeof(rc_client_server_error_t);
    case 8: return sizeof(rc_client_event_t);
    case 9: return sizeof(rc_client_all_user_progress_entry_t);
    case 10: return sizeof(rc_client_hash_library_entry_t);
    case 11: return sizeof(rc_client_game_list_entry_t);
    case 100: return offsetof(rc_api_request_t, content_type);
    case 101: return offsetof(rc_client_user_t, avatar_url);
    case 102: return offsetof(rc_client_achievement_t, unlock_time);
    case 103: return offsetof(rc_client_achievement_t, type);
    case 104: return offsetof(rc_client_achievement_t, badge_locked_url);
    case 105: return offsetof(rc_client_achievement_bucket_t, bucket_type);
    case 106: return offsetof(rc_client_event_t, server_error);
    case 107: return offsetof(rc_client_user_game_summary_t, completed_time);
    case 108: return offsetof(rc_client_hash_library_entry_t, game_id);
    case 109: return offsetof(rc_api_server_response_t, http_status_code);
    case 110: return offsetof(rc_client_game_list_entry_t, num_unsupported_hashes);
    default: return (size_t)-1;
  }
}
