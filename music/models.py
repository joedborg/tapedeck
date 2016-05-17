from django.db import models


class Artist(models.Model):
    """
    Artist model.
    """
    name = models.CharField(max_length=100)
    image = models.ImageField()
    bio = models.TextField()

    def __unicode__(self):
        return self.name


class Album(models.Model):
    """
    Album model.
    """
    artist = models.ForeignKey(Artist)
    name = models.CharField(max_length=100)
    image = models.ImageField()
    year_released = models.PositiveSmallIntegerField()

    def __unicode__(self):
        return self.name


class Song(models.Model):
    """
    Song model.
    """
    album = models.ForeignKey(Album)
    name = models.CharField(max_length=100)
    original_file = models.FileField()
    length = models.FloatField()

    def __unicode__(self):
        return self.name
