#!/bin/sh
# §11.2: the software this project is developed against is private, and nothing
# that reaches a remote may name it.
#
# The words to look for are NOT in this repository — writing them here would be
# the leak itself. They live in `.private-words`, one per line, which .gitignore
# keeps local. Without that file this script says so and fails, rather than
# passing and meaning nothing.
#
# It searches every TRACKED file and treats binaries as text, which is the hole
# that let two compiled Python files through: bytecode embeds the absolute path
# of the machine that compiled it, and this machine's path names the software.
set -eu

words=${AWASERU_PRIVATE_WORDS:-.private-words}
if [ ! -f "$words" ]; then
    echo "leak-check: no $words, so this check would pass without checking anything." >&2
    echo "Write one forbidden word per line (it is gitignored)." >&2
    exit 2
fi

status=0

# Tracked files, binaries included, in the working tree.
if git ls-files -z | xargs -0 grep -I -l -a -i -f "$words" 2>/dev/null | grep . ; then
    echo "leak-check: the files above name something §11.2 keeps private." >&2
    status=1
fi

# Commit messages, across every ref.
if git log --all --format='%H%n%s%n%b' | grep -i -f "$words" | grep . ; then
    echo "leak-check: a commit message names it." >&2
    status=1
fi

# And every blob in history, which is where the compiled bytecode hid.
for object in $(git rev-list --objects --all | cut -d' ' -f1); do
    kind=$(git cat-file -t "$object" 2>/dev/null || echo none)
    [ "$kind" = blob ] || continue
    if git cat-file blob "$object" 2>/dev/null | grep -a -q -i -f "$words"; then
        echo "leak-check: blob $object names it (in history)." >&2
        status=1
    fi
done

[ "$status" = 0 ] && echo "leak-check: clean."
exit "$status"
