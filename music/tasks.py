from celery import task


@task
def ingest_music():
    """
    Celery task to ingest a directory on indidvidual song.
    """
    # TODO complete.
