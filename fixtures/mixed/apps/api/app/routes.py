from fastapi import APIRouter

from app.service import compute

router = APIRouter()


@router.get("/compute")
def compute_endpoint():
    return compute(21)
