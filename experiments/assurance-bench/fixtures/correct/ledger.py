def apply(balance, delta):
    if balance + delta < 0:
        raise ValueError("insufficient funds")
    return balance + delta
