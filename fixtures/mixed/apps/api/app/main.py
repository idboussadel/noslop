from fastapi import FastAPI

from app.routes import router

app = FastAPI()
app.include_router(router)


def run():
    import uvicorn

    uvicorn.run(app)
