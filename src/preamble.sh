#!/usr/bin/env bash
set -e

# Sort caller arguments into JVM options vs program arguments. Anything that
# looks like a JVM flag (a -D system property, or any -X / -XX non-standard or
# advanced option: -Xmx4g, -Xss8m, -Xint, -XX:+UseZGC, ...) goes to the JVM;
# everything else passes through to the program. General purpose: callers tune
# the JVM and drive the app from one CLI.
JVM_OPTS=()
PASS_ARGS=()
USER_SET_MEM=0
DEFAULT_MEM_OPTS=('-Xms512m' '-XX:+AggressiveHeap')

for ARG in "$@"; do
  case $ARG in
    -D* | -X*)
      JVM_OPTS+=("$ARG")
      case $ARG in
        -Xms* | -Xmx* | -Xmn* | -XX:+AggressiveHeap) USER_SET_MEM=1;;
      esac
      ;;
    *)
      PASS_ARGS+=("$ARG");;
  esac
done

# Apply opinionated memory defaults only when the caller expressed no heap
# preference of their own (via _JAVA_OPTIONS, JAVA_OPTS, or a -Xms/-Xmx/-Xmn/
# -XX:+AggressiveHeap flag above).
if [ -z "${_JAVA_OPTIONS}" ] && [ -z "${JAVA_OPTS}" ] && [ "$USER_SET_MEM" -eq 0 ]; then
  JVM_OPTS=("${DEFAULT_MEM_OPTS[@]}" "${JVM_OPTS[@]}")
fi

# If not already set to some value, set MALLOC_ARENA_MAX to constrain the number of memory pools (arenas) used
# by glibc to a reasonable number. The default behaviour is to scale with the number of CPUs, which can cause
# VIRTUAL memory usage to be ~0.5GB per cpu core in the system, e.g. 32GB of a 64-core machine even when the
# heap and resident memory are only 1-4GB! See the following link for more discussion:
# https://www.ibm.com/developerworks/community/blogs/kevgrig/entry/linux_glibc_2_10_rhel_6_malloc_may_show_excessive_virtual_memory_usage?lang=en
if [ -z "${MALLOC_ARENA_MAX}" ]; then export MALLOC_ARENA_MAX=4; fi

exec java $JAVA_OPTS "${JVM_OPTS[@]}" -jar "$0" "${PASS_ARGS[@]}"
exit
