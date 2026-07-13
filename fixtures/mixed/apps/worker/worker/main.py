import click

from worker.jobs.sync import run_sync


@click.command()
def cli() -> None:
  run_sync()
