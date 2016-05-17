from celery import task


@task
def download_podcast():
    """
    Periodoc Celery task to check for podcast updates.
    """
    # TODO complete.
