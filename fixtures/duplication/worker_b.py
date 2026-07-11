def beta(person):
    lbl = person.name.strip().upper()
    pts = person.age * 2 + 10
    if pts > 50:
        return {"label": lbl, "score": pts, "tier": "gold"}
    return {"label": lbl, "score": pts, "tier": "silver"}
