def alpha(user):
    label = user.name.strip().upper()
    score = user.age * 2 + 10
    if score > 50:
        return {"label": label, "score": score, "tier": "gold"}
    return {"label": label, "score": score, "tier": "silver"}
