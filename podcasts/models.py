from django.db import models


class Publisher(models.Model):
    """
    Publisher model.
    """
    name = models.CharField(max_length=100)
    url = models.URLField()
    image = models.ImageField()
    bio = models.TextField()


class Episode(models.Model):
    """
    Episode model.
    """
    publisher = models.ForeignKey(Publisher)
    name = models.CharField(max_length=100)
    description = models.TextField()
