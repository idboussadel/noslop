from app.billing.invoice_a import subtotal_a


def total_b() -> int:
    return subtotal_a() + 5
