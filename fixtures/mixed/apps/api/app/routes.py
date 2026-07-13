from fastapi import APIRouter

from app.repos.user_repo import get_user
from app.service import compute

router = APIRouter()


@router.get("/compute")
def compute_endpoint():
    return compute(21)


@router.get("/users/{name}")
def user_endpoint(name: str):
    user = get_user(name)
    return {"name": user.name}
