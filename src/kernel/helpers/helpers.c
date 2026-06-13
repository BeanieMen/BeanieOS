

char* itoa(int value, char* buffer)
{
    int i = 0;
    int negative = 0;

    if (value == 0)
    {
        buffer[i++] = '0';
        buffer[i] = '\0';
        return buffer;
    }

    if (value < 0)
    {
        negative = 1;
        value = -value;
    }

    while (value > 0)
    {
        buffer[i++] = '0' + (value % 10);
        value /= 10;
    }

    if (negative)
    {
        buffer[i++] = '-';
    }

    buffer[i] = '\0';

    for (int j = 0, k = i - 1; j < k; j++, k--)
    {
        char temp = buffer[j];
        buffer[j] = buffer[k];
        buffer[k] = temp;
    }

    return buffer;
}

char* itoh(unsigned int value, char* buffer)
{
    static const char* hex = "0123456789ABCDEF";

    buffer[0] = '0';
    buffer[1] = 'x';

    for (int i = 0; i < 8; i++)
    {
        buffer[9 - i] = hex[value & 0xF];
        value >>= 4;
    }

    buffer[10] = '\0';

    return buffer;
}