/* Reading one value of ~/.config/infynitae/config in the edge. */
#ifndef INFYNITAE_EDGE_CONFIG_VALUE_H
#define INFYNITAE_EDGE_CONFIG_VALUE_H

#include <stdbool.h>
#include <string.h>
#include <strings.h>

/* Whether a config value (the text after `NAME=`) is the word `word`: quotes and a trailing
 * comment are not part of it. strstr matched `EDGE_POSITION="bottom" # not top` as top. */
static bool config_value_is(const char *v, const char *word) {
    while (*v == ' ' || *v == '\t') v++;
    if (*v == '"' || *v == '\'') v++;
    size_t n = strlen(word);
    if (strncasecmp(v, word, n) != 0) return false;
    char end = v[n];
    return end == '\0' || end == '"' || end == '\'' || end == ' ' || end == '\t' || end == '\n' || end == '\r' ||
           end == '#';
}

#endif
