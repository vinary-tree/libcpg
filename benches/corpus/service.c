#include <stdbool.h>
#include <stddef.h>

typedef struct {
    int account;
    long amount;
} entry_t;

typedef struct {
    entry_t *entries;
    size_t length;
    size_t capacity;
} ledger_t;

long balance(const ledger_t *ledger, int account) {
    long total = 0;
    for (size_t index = 0; index < ledger->length; ++index) {
        if (ledger->entries[index].account == account) {
            total += ledger->entries[index].amount;
        }
    }
    return total;
}

bool transfer(ledger_t *ledger, int sender, int receiver, long amount) {
    if (amount <= 0 || balance(ledger, sender) < amount ||
        ledger->length + 2 > ledger->capacity) {
        return false;
    }
    ledger->entries[ledger->length++] = (entry_t){sender, -amount};
    ledger->entries[ledger->length++] = (entry_t){receiver, amount};
    return true;
}
